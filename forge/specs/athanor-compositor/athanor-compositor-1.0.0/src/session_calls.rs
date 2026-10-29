//! The session-bus worker of the key bindings (doc_own_stack.md OS13, plan D5 to D7): one
//! thread, `session-calls`, makes the calls a bound key asks for, each a method without
//! arguments, and asks the user manager to run its commands as transient units, waiting for
//! each reply there, so the event loop never waits on the bus. The loop hands it requests
//! through a bounded queue; a full queue drops the request with a warning. A name with a
//! service file is started by the bus at its first call. A key forks nothing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;
use zbus::zvariant::Value;

/// A method of the session bus without arguments, and how a key's request for it is queued.
#[derive(Debug, PartialEq)]
pub(crate) struct Call {
    pub destination: &'static str,
    pub path: &'static str,
    pub interface: &'static str,
    pub method: &'static str,
    /// Whether the key acts while the session is locked; a request for a call without it,
    /// queued before a lock, is dropped.
    pub while_locked: bool,
    /// Whether requests for the call queued behind one that was just sent are dropped: a
    /// toggle pressed again while its first call waits is not replayed.
    pub coalesce: bool,
}

/// Super alone: the launcher shows, or hides when shown (LA8,
/// `athanor-launcher-1.0.0/src/bus.rs`).
pub(crate) const LAUNCHER: Call = Call {
    destination: "os.athanor.Launcher1",
    path: "/os/athanor/Launcher1",
    interface: "os.athanor.Launcher1",
    method: "Show",
    while_locked: false,
    coalesce: true,
};

/// Super+C: the control center opens, or closes when open (CC9,
/// `athanor-control-center-1.0.0/src/bus.rs`).
pub(crate) const CONTROL_CENTER: Call = Call {
    destination: "os.athanor.ControlCenter1",
    path: "/os/athanor/ControlCenter1",
    interface: "os.athanor.ControlCenter1",
    method: "Toggle",
    while_locked: false,
    coalesce: true,
};

/// A command the user manager runs as a transient unit (plan D6): a short name for the unit
/// and a fixed argument vector, whose first element is an absolute path; no shell reads it.
#[derive(Debug, PartialEq)]
pub(crate) struct Command {
    pub name: &'static str,
    pub argv: &'static [&'static str],
    /// Whether the key acts while the session is locked, as `Call::while_locked`.
    pub while_locked: bool,
    /// Whether a repeat queued while the command's request waits is dropped, as
    /// `Call::coalesce`: a toggle or a dialog, never a step.
    pub coalesce: bool,
}

/// `XF86AudioMute`, as cosmic-comp's session runs it (`defaults:117`), until `ToggleMute`
/// (doc_osd.md OD3).
pub(crate) const MUTE: Command = Command {
    name: "mute",
    argv: &[
        "/usr/bin/wpctl",
        "set-mute",
        "@DEFAULT_AUDIO_SINK@",
        "toggle",
    ],
    while_locked: true,
    coalesce: true,
};

/// `XF86AudioMicMute` (`defaults:118`), until `ToggleMicMute`.
pub(crate) const MIC_MUTE: Command = Command {
    name: "mic-mute",
    argv: &[
        "/usr/bin/wpctl",
        "set-mute",
        "@DEFAULT_AUDIO_SOURCE@",
        "toggle",
    ],
    while_locked: false,
    coalesce: true,
};

/// `XF86AudioPlay` (`defaults:121`), until `PlayPause` (OD3).
pub(crate) const PLAY_PAUSE: Command = Command {
    name: "play-pause",
    argv: &["/usr/bin/playerctl", "play-pause"],
    while_locked: false,
    coalesce: true,
};

/// `XF86AudioNext` (`defaults:122`), until `Next` (OD3).
pub(crate) const NEXT: Command = Command {
    name: "next",
    argv: &["/usr/bin/playerctl", "next"],
    while_locked: false,
    coalesce: false,
};

/// `XF86AudioPrev` (`defaults:123`), until `Previous` (OD3).
pub(crate) const PREVIOUS: Command = Command {
    name: "previous",
    argv: &["/usr/bin/playerctl", "previous"],
    while_locked: false,
    coalesce: false,
};

/// Super+Shift+Escape: cosmic-osd's log-out dialog (`defaults:3`), until
/// `os.athanor.Osd1.AskLogOut` (OD3, OD15).
pub(crate) const LOG_OUT: Command = Command {
    name: "log-out",
    argv: &["/usr/bin/cosmic-osd", "log-out"],
    while_locked: false,
    coalesce: true,
};

/// `XF86PowerOff`: cosmic-osd's power dialog, as cosmic-comp's `system_actions` runs it
/// (`defaults:124`), until `AskPowerOff` (OD3).
pub(crate) const POWER_OFF: Command = Command {
    name: "power-off",
    argv: &["/usr/bin/cosmic-osd", "shutdown"],
    while_locked: false,
    coalesce: true,
};

/// What a key asks the worker for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Job {
    Call(&'static Call),
    Run(&'static Command),
}

impl Job {
    fn while_locked(self) -> bool {
        match self {
            Self::Call(call) => call.while_locked,
            Self::Run(command) => command.while_locked,
        }
    }

    fn coalesce(self) -> bool {
        match self {
            Self::Call(call) => call.coalesce,
            Self::Run(command) => command.coalesce,
        }
    }
}

/// A job a key asked for, stamped with the lock epoch it was asked in (`State::lock_epoch`).
#[derive(Debug, PartialEq)]
pub(crate) struct Request {
    pub job: Job,
    pub epoch: u64,
}

/// The requests the queue holds; the worker holds as many again that it took to coalesce.
const QUEUE: usize = 8;

/// How long a call waits for its reply, as logind's `Lock` (`lock.rs`).
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";

/// The loop's end of the worker's queue.
pub(crate) struct SessionCalls {
    queue: SyncSender<Request>,
}

impl SessionCalls {
    /// Queues `request` without waiting: a full queue, or a worker that has ended, drops it
    /// with a warning, and the next press asks again.
    pub(crate) fn send(&self, request: Request) {
        let job = request.job;
        match self.queue.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    ?job,
                    "the session-bus queue is full; the key's job is dropped"
                );
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!(
                    ?job,
                    "the session-bus worker has ended; the key's job is dropped"
                );
            }
        }
    }
}

/// Starts the worker, which reads the lock epoch from `epoch`; `None`, logged, when the thread
/// cannot start, and the keys of the bus then do nothing.
pub(crate) fn start(epoch: Arc<AtomicU64>) -> Option<SessionCalls> {
    let (queue, requests) = sync_channel(QUEUE);
    let spawned = std::thread::Builder::new()
        .name("session-calls".into())
        .spawn(move || work(&requests, &epoch, &mut SessionBus::default()));
    match spawned {
        Ok(_) => Some(SessionCalls { queue }),
        Err(error) => {
            tracing::error!(%error, "the session-bus worker did not start; the keys of the bus do nothing");
            None
        }
    }
}

/// The unit a command runs as: `app-athanor\x2dcompositor-<name>-<pid>x<n>.service`, systemd's
/// naming of an application's units, with `-` inside a component escaped as `\x2d`, `n` the
/// command's count in this run of the compositor.
fn unit_name(name: &str, pid: u32, n: u64) -> String {
    format!(
        r"app-athanor\x2dcompositor-{}-{pid}x{n}.service",
        name.replace('-', r"\x2d")
    )
}

/// The properties of the transient service that runs `argv`: its first element is executed
/// with the vector as is, by the user manager, which starts no shell, and the program gains no
/// privilege. With `Type=exec` a program that cannot be executed fails its unit, which the user
/// manager's journal records; the call's reply only says the job was queued.
fn transient_properties(argv: &[&str]) -> Vec<(&'static str, Value<'static>)> {
    let args: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
    let program = args.first().cloned().unwrap_or_default();
    vec![
        ("ExecStart", Value::from(vec![(program, args, false)])),
        ("Type", Value::from("exec")),
        ("NoNewPrivileges", Value::from(true)),
        ("CollectMode", Value::from("inactive-or-failed")),
        ("Slice", Value::from("app.slice")),
    ]
}

/// Where the worker sends a job; the bus, or a test's record.
trait Bus {
    /// Opens the connection when there is none, before the job's epoch is read: false, logged,
    /// when it cannot be opened.
    fn connect(&mut self) -> bool;
    fn call(&mut self, call: &Call);
    fn run(&mut self, unit: &str, command: &Command);
}

/// The session bus, connected at the first call and again after an error.
#[derive(Default)]
struct SessionBus {
    connection: Option<zbus::blocking::Connection>,
}

impl SessionBus {
    /// Sends `send` on the connection `Bus::connect` opened; it is dropped, and opened again
    /// before the next job, when the call failed for another reason than an error reply.
    fn send(
        &mut self,
        send: impl FnOnce(&zbus::blocking::Connection) -> zbus::Result<zbus::Message>,
    ) -> zbus::Result<()> {
        let connection = self
            .connection
            .take()
            .ok_or_else(|| zbus::Error::Failure("no session bus connection".into()))?;
        match send(&connection) {
            Ok(_) => {
                self.connection = Some(connection);
                Ok(())
            }
            Err(error @ zbus::Error::MethodError(..)) => {
                self.connection = Some(connection);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

impl Bus for SessionBus {
    fn connect(&mut self) -> bool {
        if self.connection.is_some() {
            return true;
        }
        let opened = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.method_timeout(CALL_TIMEOUT).build());
        match opened {
            Ok(connection) => {
                self.connection = Some(connection);
                true
            }
            Err(error) => {
                tracing::warn!(%error, "the session bus cannot be reached; the key's job is dropped");
                false
            }
        }
    }

    fn call(&mut self, call: &Call) {
        let sent = self.send(|connection| {
            connection.call_method(
                Some(call.destination),
                call.path,
                Some(call.interface),
                call.method,
                &(),
            )
        });
        if let Err(error) = sent {
            tracing::warn!(
                %error,
                destination = call.destination,
                method = call.method,
                "a key's call on the session bus failed"
            );
        }
    }

    fn run(&mut self, unit: &str, command: &Command) {
        let auxiliary: Vec<(&str, Vec<(&str, Value)>)> = Vec::new();
        let body = (unit, "fail", transient_properties(command.argv), auxiliary);
        let sent = self.send(|connection| {
            connection.call_method(
                Some(SYSTEMD),
                SYSTEMD_PATH,
                Some(MANAGER),
                "StartTransientUnit",
                &body,
            )
        });
        match sent {
            Ok(()) => tracing::debug!(unit, "the user manager queued a key's command"),
            Err(error) => tracing::warn!(
                %error,
                unit,
                command = command.name,
                "the user manager did not start a key's command"
            ),
        }
    }
}

/// Sends each request of `requests` in order until the loop's end goes: a request asked
/// before the session locked is dropped unless its key acts while locked, and after each
/// call the run of identical requests queued behind it is dropped when its call coalesces.
fn work(requests: &Receiver<Request>, epoch: &AtomicU64, bus: &mut impl Bus) {
    let pid = std::process::id();
    let mut runs = 0;
    let mut pending = VecDeque::new();
    loop {
        let request = match pending.pop_front() {
            Some(request) => request,
            None => match requests.recv() {
                Ok(request) => request,
                Err(_) => return,
            },
        };
        let Request { job, epoch: asked } = request;
        // The connection is opened first, which may take a while: a lock that starts meanwhile
        // still drops the job.
        if !bus.connect() {
            continue;
        }
        if asked != epoch.load(Ordering::SeqCst) && !job.while_locked() {
            tracing::info!(?job, "the session locked after the key; its job is dropped");
            continue;
        }
        match job {
            Job::Call(call) => bus.call(call),
            Job::Run(command) => {
                runs += 1;
                bus.run(&unit_name(command.name, pid, runs), command);
            }
        }
        while pending.len() < QUEUE {
            match requests.try_recv() {
                Ok(request) => pending.push_back(request),
                Err(_) => break,
            }
        }
        if job.coalesce() {
            // A press from another lock epoch is not a repeat: a mute on the lock surface after
            // one sent before the lock is a second press.
            while pending
                .front()
                .is_some_and(|next| next.job == job && next.epoch == asked)
            {
                pending.pop_front();
                tracing::debug!(
                    ?job,
                    "a repeat queued while the key's job waited is dropped"
                );
            }
        }
    }
}

#[cfg(test)]
impl SessionCalls {
    /// A queue no worker reads: the test reads it.
    pub(crate) fn detached() -> (Self, Receiver<Request>) {
        let (queue, requests) = sync_channel(QUEUE);
        (Self { queue }, requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the worker sent, by method or command name; `lock_on` locks the session while
    /// that method's call waits for its reply, or, named `CONNECT`, while each connection step
    /// waits.
    struct Record<'a> {
        sent: Vec<&'static str>,
        lock_on: Option<(&'static str, &'a AtomicU64)>,
    }

    const CONNECT: &str = "connect";

    impl Bus for Record<'_> {
        fn connect(&mut self) -> bool {
            if let Some((CONNECT, epoch)) = self.lock_on {
                epoch.fetch_add(1, Ordering::SeqCst);
            }
            true
        }

        fn call(&mut self, call: &Call) {
            self.sent.push(call.method);
            if let Some((method, epoch)) = self.lock_on {
                if method == call.method {
                    epoch.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        fn run(&mut self, unit: &str, command: &Command) {
            assert!(unit.contains(&command.name.replace('-', r"\x2d")), "{unit}");
            self.sent.push(command.name);
        }
    }

    /// Runs the worker over `requests`, the lock epoch then being `now`, the session locking
    /// while `lock_on`'s call waits: what it sent.
    fn worked(
        requests: Vec<Request>,
        now: u64,
        lock_on: Option<&'static str>,
    ) -> Vec<&'static str> {
        let (queue, receiver) = sync_channel(requests.len());
        for request in requests {
            queue.try_send(request).expect("room");
        }
        drop(queue);
        let epoch = AtomicU64::new(now);
        let mut bus = Record {
            sent: Vec::new(),
            lock_on: lock_on.map(|method| (method, &epoch)),
        };
        work(&receiver, &epoch, &mut bus);
        bus.sent
    }

    fn at(job: Job, epoch: u64) -> Request {
        Request { job, epoch }
    }

    #[test]
    fn the_epoch_is_read_after_the_connection_is_opened() {
        assert_eq!(
            worked(
                vec![at(Job::Run(&LOG_OUT), 0), at(Job::Run(&MUTE), 0)],
                0,
                Some(CONNECT)
            ),
            ["mute"],
            "a lock that starts while the bus connects drops the log-out"
        );
    }

    #[test]
    fn a_repeat_from_another_lock_epoch_is_not_coalesced() {
        let mute = |epoch| at(Job::Run(&MUTE), epoch);
        assert_eq!(worked(vec![mute(0), mute(1)], 1, None), ["mute", "mute"]);
        assert_eq!(worked(vec![mute(1), mute(1)], 1, None), ["mute"]);
    }

    #[test]
    fn the_launcher_and_control_center_calls_are_their_programs() {
        let launcher = include_str!("../../../athanor-launcher/athanor-launcher-1.0.0/src/bus.rs");
        let control_center =
            include_str!("../../../athanor-control-center/athanor-control-center-1.0.0/src/bus.rs");
        for (call, source, command) in [
            (
                &LAUNCHER,
                launcher,
                format!(
                    "--dest {} --object-path {} --method {}.{}",
                    LAUNCHER.destination, LAUNCHER.path, LAUNCHER.interface, LAUNCHER.method
                ),
            ),
            (
                &CONTROL_CENTER,
                control_center,
                format!(
                    "call {} {} {} {}",
                    CONTROL_CENTER.destination,
                    CONTROL_CENTER.path,
                    CONTROL_CENTER.interface,
                    CONTROL_CENTER.method
                ),
            ),
        ] {
            assert!(source.contains(&command), "{call:?}: {command}");
        }
    }

    #[test]
    fn a_full_queue_drops_without_waiting() {
        let (calls, requests) = SessionCalls::detached();
        for _ in 0..QUEUE + 1 {
            calls.send(at(Job::Call(&LAUNCHER), 0));
        }
        assert_eq!(requests.try_iter().count(), QUEUE, "the ninth is dropped");
        let (calls, requests) = SessionCalls::detached();
        drop(requests);
        calls.send(at(Job::Call(&LAUNCHER), 0));
    }

    #[test]
    fn a_request_from_before_the_lock_is_dropped_unless_it_acts_locked() {
        assert_eq!(
            worked(
                vec![
                    at(Job::Call(&LAUNCHER), 0),
                    at(Job::Run(&MUTE), 0),
                    at(Job::Call(&CONTROL_CENTER), 1),
                ],
                1,
                None
            ),
            ["mute", "Toggle"],
            "the Show asked before the lock is not sent after it"
        );
        // A log-out queued behind a slow Show(), then Super+Escape: the session locks while
        // the Show waits, and the log-out is not run on the lock.
        assert_eq!(
            worked(
                vec![
                    at(Job::Call(&LAUNCHER), 0),
                    at(Job::Run(&LOG_OUT), 0),
                    at(Job::Run(&MUTE), 0),
                ],
                0,
                Some("Show")
            ),
            ["Show", "mute"]
        );
    }

    #[test]
    fn only_the_run_behind_a_sent_toggle_is_coalesced() {
        let launcher = || at(Job::Call(&LAUNCHER), 0);
        assert_eq!(
            worked(
                vec![
                    launcher(),
                    launcher(),
                    at(Job::Call(&CONTROL_CENTER), 0),
                    launcher(),
                ],
                0,
                None
            ),
            ["Show", "Toggle", "Show"],
            "Show, Toggle, Show: the third Show is still sent"
        );
        let next = || at(Job::Run(&NEXT), 0);
        assert_eq!(
            worked(vec![next(), next(), next()], 0, None),
            ["next", "next", "next"],
            "a step is run every time"
        );
        let mute = || at(Job::Run(&MUTE), 0);
        assert_eq!(
            worked(
                vec![mute(), mute(), at(Job::Run(&NEXT), 0), mute()],
                0,
                None
            ),
            ["mute", "next", "mute"],
            "a toggle's repeat is dropped, as a call's"
        );
    }

    #[test]
    fn the_unit_name_is_escaped_and_unique() {
        assert_eq!(
            unit_name("mute", 4242, 1),
            r"app-athanor\x2dcompositor-mute-4242x1.service"
        );
        assert_eq!(
            unit_name("log-out", 4242, 2),
            r"app-athanor\x2dcompositor-log\x2dout-4242x2.service"
        );
        assert_ne!(unit_name("mute", 4242, 1), unit_name("mute", 4242, 2));
    }

    #[test]
    fn the_properties_run_the_vector_without_a_shell() {
        for command in [
            &MUTE,
            &MIC_MUTE,
            &PLAY_PAUSE,
            &NEXT,
            &PREVIOUS,
            &LOG_OUT,
            &POWER_OFF,
        ] {
            assert!(
                command
                    .argv
                    .first()
                    .is_some_and(|program| program.starts_with('/')),
                "{command:?} runs an absolute path"
            );
            assert!(
                command
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{command:?}: a name of a unit's component"
            );
            let properties = transient_properties(command.argv);
            let argv: Vec<String> = command.argv.iter().map(|arg| arg.to_string()).collect();
            assert_eq!(
                properties,
                [
                    (
                        "ExecStart",
                        Value::from(vec![(argv[0].clone(), argv.clone(), false)])
                    ),
                    ("Type", Value::from("exec")),
                    ("NoNewPrivileges", Value::from(true)),
                    ("CollectMode", Value::from("inactive-or-failed")),
                    ("Slice", Value::from("app.slice")),
                ]
            );
            assert!(
                !format!("{properties:?}").contains("/bin/sh"),
                "{command:?}: no shell"
            );
        }
    }
}
