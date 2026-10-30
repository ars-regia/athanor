//! Orca's keyboard monitor (doc_own_stack.md OS4, doc_accessibility.md AX4):
//! `org.freedesktop.a11y.KeyboardMonitor` at `/org/freedesktop/a11y/Manager`, under the name
//! `org.freedesktop.a11y.Manager` on the session bus, the interface libatspi's `AtspiDevice`
//! calls and cosmic-comp serves. A client watches the keyboard, grabs it whole, or grabs single
//! keys; a watcher is sent every key as a `KeyEvent` signal addressed to it alone, and a grabbed
//! press, with its release, reaches no other client.
//!
//! Admission (AX4): a call counts only from the owner of `org.gnome.Orca.KeyboardMonitor` whose
//! process lies in `orca.service`'s cgroup and runs an executable under `/usr` that is the very
//! file at that path (`identity.rs`'s unit and file checks). The process is read through the
//! pidfd the bus's `GetConnectionCredentials` returns as `ProcessFD`; from a bus without it, through
//! a pidfd opened on its `ProcessID`, whose connection must still name that pid once it is read
//! (AX5's reuse check). Orca is a Python script, so its executable is the interpreter: the unit
//! is what tells Orca from another program under `/usr`. Every call is admitted anew, and every
//! change of the name's owner, or of an admitted client's connection, admits each client again
//! and forgets those that fail. A refused call is answered as an accepted one and changes
//! nothing; the journal names it.
//!
//! Threads: `keyboard-monitor` admits the calls and the changes of names one at a time, in the
//! order the bus sent them, and hands the event loop an `Update` through a channel;
//! `keyboard-monitor-names` reads the changes of names; `keyboard-monitor-signals` sends the
//! signals the loop queues (`Monitor::observe`). The loop never waits on the bus.

use crate::identity::{self, Process};
use crate::state::State;
use crate::Error;
use smithay::input::keyboard::{KeysymHandle, ModifiersState};
use smithay::reexports::calloop::{channel, LoopHandle};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::os::fd::AsFd;
use std::path::{Component, Path};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use zbus::blocking::fdo::DBusProxy;
use zbus::fdo::ConnectionCredentials;
use zbus::message::Header;

const PATH: &str = "/org/freedesktop/a11y/Manager";
const NAME: &str = "org.freedesktop.a11y.Manager";
const INTERFACE: &str = "org.freedesktop.a11y.KeyboardMonitor";
/// The name Orca 49 claims for the monitor (`Atspi.Device.new_full("org.gnome.Orca")`).
const ORCA: &str = "org.gnome.Orca.KeyboardMonitor";
/// The unit Orca runs as (AX3).
const ORCA_UNIT: &str = "orca.service";
/// In a key grab's modifiers, the bit of the first virtual modifier `SetKeyGrabs` names; the
/// bits below are the real modifiers (at-spi2-core's `ATSPI_DEVICE_A11Y_MANAGER_VIRTUAL_MOD_START`).
const VIRTUAL_MOD_START: u32 = 15;
/// The calls and the signals each queue holds; a full queue drops a call, or a key's signal,
/// with a warning.
const QUEUE: usize = 256;

/// A key a client grabs: the key's keysym with exactly these real modifiers held and these
/// virtual modifiers active.
#[derive(Debug, PartialEq)]
struct KeyGrab {
    keysym: u32,
    mods: u32,
    virtual_mods: BTreeSet<u32>,
}

impl KeyGrab {
    /// The grab `SetKeyGrabs` names as `(keysym, modifiers)`, where bit `VIRTUAL_MOD_START + i`
    /// of the modifiers stands for the `i`th of `virtual_mods`.
    fn new(virtual_mods: &[u32], keysym: u32, raw_mods: u32) -> Self {
        let virtual_mods = virtual_mods
            .iter()
            .zip(VIRTUAL_MOD_START..u32::BITS)
            .filter(|(_, bit)| raw_mods & (1 << bit) != 0)
            .map(|(keysym, _)| *keysym)
            .collect();
        Self {
            keysym,
            mods: raw_mods & ((1 << VIRTUAL_MOD_START) - 1),
            virtual_mods,
        }
    }
}

/// What an admitted client asked for.
#[derive(Debug, Default)]
struct Client {
    watched: bool,
    grabbed: bool,
    /// The keysyms that act as its modifiers (Orca's Insert or Caps Lock): their press reaches
    /// no client.
    virtual_mods: Vec<u32>,
    key_grabs: Vec<KeyGrab>,
}

/// A call, or the end of a client.
#[derive(Debug, PartialEq)]
pub(crate) enum Change {
    Watch(bool),
    Grab(bool),
    /// The virtual modifiers and the `(keysym, modifiers)` pairs of `SetKeyGrabs`.
    KeyGrabs(Vec<u32>, Vec<(u32, u32)>),
    Forget,
}

/// A change of a client, admitted, for the event loop.
#[derive(Debug, PartialEq)]
pub(crate) struct Update {
    /// The client's unique name on the bus.
    pub client: String,
    pub change: Change,
}

/// A `KeyEvent` signal, addressed to one client: `(released, state, keysym, unichar, keycode)`
/// as the interface defines them.
#[derive(Debug, PartialEq)]
pub(crate) struct KeyEvent {
    pub destination: String,
    pub released: bool,
    /// The modifiers held (xkb's depressed mask).
    pub state: u32,
    pub keysym: u32,
    /// The key's character, 0 for none.
    pub unichar: u32,
    /// The xkb keycode, evdev's plus 8.
    pub keycode: u16,
}

/// The event loop's side of the monitor: the admitted clients and the virtual modifiers held.
#[derive(Debug, Default)]
pub(crate) struct Monitor {
    clients: HashMap<String, Client>,
    active: BTreeSet<u32>,
    /// Where the signals go; none headless, a test's queue in the fixture.
    pub events: Option<SyncSender<KeyEvent>>,
}

impl Monitor {
    pub(crate) fn apply(&mut self, update: Update) {
        let Update { client, change } = update;
        match change {
            Change::Forget => {
                self.clients.remove(&client);
            }
            Change::Watch(on) => self.clients.entry(client).or_default().watched = on,
            Change::Grab(on) => self.clients.entry(client).or_default().grabbed = on,
            Change::KeyGrabs(virtual_mods, keystrokes) => {
                let client = self.clients.entry(client).or_default();
                client.key_grabs = keystrokes
                    .into_iter()
                    .map(|(keysym, mods)| KeyGrab::new(&virtual_mods, keysym, mods))
                    .collect();
                client.virtual_mods = virtual_mods;
            }
        }
        // A virtual modifier no client names any more holds nothing back.
        let clients = &self.clients;
        self.active
            .retain(|keysym| clients.values().any(|c| c.virtual_mods.contains(keysym)));
    }

    /// Whether one of `client`'s key grabs is the key `keysym` with `mods` held.
    fn grabs_key(&self, client: &Client, mods: u32, keysym: u32) -> bool {
        client.key_grabs.iter().any(|grab| {
            grab.keysym == keysym && grab.mods == mods && grab.virtual_mods == self.active
        })
    }

    /// Queues the key for each client that watches the keyboard, grabs it whole, or grabs this
    /// key: every key, bound or not, aimed at any surface, the lock's included.
    pub(crate) fn observe(
        &self,
        modifiers: &ModifiersState,
        handle: &KeysymHandle<'_>,
        released: bool,
    ) {
        let Some(events) = &self.events else {
            return;
        };
        let keysym = handle.modified_sym();
        let mods = modifiers.serialized.depressed;
        for (name, client) in &self.clients {
            if !(client.watched || client.grabbed || self.grabs_key(client, mods, keysym.raw())) {
                continue;
            }
            let event = KeyEvent {
                destination: name.clone(),
                released,
                state: mods,
                keysym: keysym.raw(),
                unichar: keysym.key_char().map_or(0, u32::from),
                keycode: u16::try_from(handle.raw_code().raw()).unwrap_or_default(),
            };
            match events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    tracing::warn!(
                        "the keyboard monitor's queue is full; a key is not sent to Orca"
                    );
                }
                Err(TrySendError::Disconnected(_)) => {
                    tracing::warn!("the keyboard monitor has ended; a key is not sent to Orca");
                }
            }
        }
    }

    /// Whether a press is kept from the focused client: a virtual modifier, which becomes
    /// active, or a key a client grabs, or any key while a client grabs the keyboard. The
    /// caller asks only when no binding took the key and the session is not locked.
    pub(crate) fn takes(&mut self, modifiers: &ModifiersState, handle: &KeysymHandle<'_>) -> bool {
        let keysym = handle.modified_sym().raw();
        if self
            .clients
            .values()
            .any(|c| c.virtual_mods.contains(&keysym))
        {
            self.active.insert(keysym);
            return true;
        }
        let mods = modifiers.serialized.depressed;
        self.clients
            .values()
            .any(|c| c.grabbed || self.grabs_key(c, mods, keysym))
    }

    /// A key let go: whether it was an active virtual modifier, which is no longer.
    pub(crate) fn released(&mut self, handle: &KeysymHandle<'_>) -> bool {
        self.active.remove(&handle.modified_sym().raw())
    }
}

/// Whether the client `client`, with `credentials` from the bus, is Orca while `owner` owns
/// Orca's name: the pid it was read through, or why not.
pub(crate) fn check(
    client: &str,
    owner: &str,
    credentials: &ConnectionCredentials,
    process: &dyn Process,
) -> Result<i32, Error> {
    if client != owner {
        return Err(format!("{client} does not own {ORCA}").into());
    }
    let opened;
    let pidfd = match credentials.process_fd() {
        Some(fd) => fd.as_fd(),
        None => {
            let pid = credentials
                .process_id()
                .ok_or("the bus names neither a pidfd nor a pid")?;
            opened = identity::pidfd_open(pid)?;
            opened.as_fd()
        }
    };
    let pid = identity::pidfd_pid(pidfd)?;
    let cgroup = process.cgroup(pid)?;
    if identity::unit(&cgroup) != Some(ORCA_UNIT) {
        return Err(format!("{client} is not in {ORCA_UNIT}").into());
    }
    let executable = process.executable(pid)?;
    let under_usr = executable.starts_with("/usr")
        && executable != Path::new("/usr")
        && !executable.components().any(|c| c == Component::ParentDir);
    if !under_usr || process.file(&executable)? != process.executable_file(pid)? {
        return Err(format!("{client} runs {} from outside /usr", executable.display()).into());
    }
    if identity::pidfd_pid(pidfd)? != pid {
        return Err(format!("{client} exited while it was read").into());
    }
    Ok(pid)
}

/// Asks the bus who owns Orca's name and who `client` is, and `check`s them.
fn admit(bus: &DBusProxy<'_>, client: &str, process: &dyn Process) -> Result<(), Error> {
    let owner = bus.get_name_owner(ORCA.try_into()?)?;
    let credentials = bus.get_connection_credentials(client.try_into()?)?;
    let pid = check(client, owner.as_str(), &credentials, process)?;
    if credentials.process_fd().is_none() {
        // The pidfd was opened after the bus named the pid: the connection, still open with that
        // pid, shows the pid was not reused in between.
        let again = bus.get_connection_credentials(client.try_into()?)?;
        if again.process_id().map(i64::from) != Some(i64::from(pid)) {
            return Err(format!("{client}'s pid changed while it was read").into());
        }
    }
    Ok(())
}

/// What the `keyboard-monitor` thread admits, in order.
enum Job {
    Call(String, Change),
    /// The owner of a name changed, or a connection ended.
    Renamed(String),
    /// The changes of names can no longer be read.
    Blind,
}

/// The interface, on zbus's threads: it queues each call for admission.
struct Interface {
    jobs: SyncSender<Job>,
}

impl Interface {
    fn ask(&self, header: &Header<'_>, change: Change) {
        let Some(client) = header.sender() else {
            return;
        };
        let job = Job::Call(client.to_string(), change);
        if let Err(error) = self.jobs.try_send(job) {
            tracing::warn!(%client, %error, "a keyboard monitor call is dropped");
        }
    }
}

#[zbus::interface(name = "org.freedesktop.a11y.KeyboardMonitor")]
impl Interface {
    fn grab_keyboard(&self, #[zbus(header)] header: Header<'_>) {
        self.ask(&header, Change::Grab(true));
    }

    fn ungrab_keyboard(&self, #[zbus(header)] header: Header<'_>) {
        self.ask(&header, Change::Grab(false));
    }

    fn watch_keyboard(&self, #[zbus(header)] header: Header<'_>) {
        self.ask(&header, Change::Watch(true));
    }

    fn unwatch_keyboard(&self, #[zbus(header)] header: Header<'_>) {
        self.ask(&header, Change::Watch(false));
    }

    fn set_key_grabs(
        &self,
        #[zbus(header)] header: Header<'_>,
        virtual_mods: Vec<u32>,
        keystrokes: Vec<(u32, u32)>,
    ) {
        self.ask(&header, Change::KeyGrabs(virtual_mods, keystrokes));
    }
}

/// Starts the monitor: the queue the event loop sends its signals to, or `None`, logged, when
/// it cannot start. The bus is reached on the monitor's thread; when it cannot be, or the name
/// is taken, the journal says so and Orca's own commands do not reach Orca.
pub(crate) fn start(handle: &LoopHandle<'static, State>) -> Option<SyncSender<KeyEvent>> {
    let (updates, received) = channel::channel();
    let inserted = handle.insert_source(received, |event, _, state| {
        if let channel::Event::Msg(update) = event {
            state.keyboard_monitor.apply(update);
        }
    });
    if let Err(error) = inserted {
        tracing::error!(%error, "the keyboard monitor's channel was not added to the loop");
        return None;
    }
    let (events, queued) = sync_channel(QUEUE);
    let spawned = std::thread::Builder::new()
        .name("keyboard-monitor".into())
        .spawn(move || {
            if let Err(error) = serve(&updates, queued) {
                tracing::error!(%error, "the keyboard monitor has stopped; Orca's own commands do not reach Orca");
            }
        });
    match spawned {
        Ok(_) => Some(events),
        Err(error) => {
            tracing::error!(%error, "the keyboard monitor did not start");
            None
        }
    }
}

/// Serves the interface and admits its calls until the compositor ends, or the changes of names
/// can no longer be read: then every client is forgotten and the interface withdrawn.
fn serve(updates: &channel::Sender<Update>, queued: Receiver<KeyEvent>) -> Result<(), Error> {
    let connection = zbus::blocking::Connection::session()?;
    let bus = DBusProxy::new(&connection)?;
    // Read from before the first call can be admitted, so that no change is missed.
    let renames = bus.receive_name_owner_changed()?;
    let (jobs, admissions) = sync_channel(QUEUE);
    let watcher = jobs.clone();
    std::thread::Builder::new()
        .name("keyboard-monitor-names".into())
        .spawn(move || {
            for rename in renames {
                let job = match rename.args() {
                    Ok(args) => Job::Renamed(args.name().to_string()),
                    Err(error) => {
                        tracing::error!(%error, "a change of a name on the bus cannot be read");
                        Job::Blind
                    }
                };
                // A blocking send: a change of names is never dropped.
                if watcher.send(job).is_err() {
                    return;
                }
            }
            if watcher.send(Job::Blind).is_err() {
                tracing::debug!("the keyboard monitor ended before its reader of names");
            }
        })?;
    let emitter = connection.clone();
    std::thread::Builder::new()
        .name("keyboard-monitor-signals".into())
        .spawn(move || {
            for event in queued {
                let body = (
                    event.released,
                    event.state,
                    event.keysym,
                    event.unichar,
                    event.keycode,
                );
                let destination = Some(event.destination.as_str());
                if let Err(error) =
                    emitter.emit_signal(destination, PATH, INTERFACE, "KeyEvent", &body)
                {
                    tracing::warn!(%error, "a key was not sent to Orca");
                }
            }
        })?;
    connection.object_server().at(PATH, Interface { jobs })?;
    let reply =
        connection.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
    if reply != zbus::fdo::RequestNameReply::PrimaryOwner {
        return Err(format!("{NAME} is owned by another process ({reply:?})").into());
    }
    tracing::info!("serving Orca's keyboard monitor");

    let process = identity::Proc;
    let mut admitted: HashSet<String> = HashSet::new();
    for job in admissions {
        match job {
            Job::Call(client, change) => match admit(&bus, &client, &process) {
                Ok(()) => {
                    admitted.insert(client.clone());
                    updates.send(Update { client, change })?;
                }
                Err(error) => {
                    tracing::warn!(%client, %error, "a keyboard monitor call is refused");
                    if admitted.remove(&client) {
                        updates.send(Update {
                            client,
                            change: Change::Forget,
                        })?;
                    }
                }
            },
            Job::Renamed(name) if name == ORCA || admitted.contains(&name) => {
                let failed: Vec<String> = admitted
                    .iter()
                    .filter(|client| match admit(&bus, client, &process) {
                        Ok(()) => false,
                        Err(error) => {
                            tracing::info!(%client, %error, "a keyboard monitor client is forgotten");
                            true
                        }
                    })
                    .cloned()
                    .collect();
                for client in failed {
                    admitted.remove(&client);
                    updates.send(Update {
                        client,
                        change: Change::Forget,
                    })?;
                }
            }
            Job::Renamed(_) => {}
            Job::Blind => {
                for client in admitted.drain() {
                    updates.send(Update {
                        client,
                        change: Change::Forget,
                    })?;
                }
                connection.object_server().remove::<Interface, _>(PATH)?;
                connection.release_name(NAME)?;
                return Err("the changes of names on the bus can no longer be read".into());
            }
        }
    }
    Ok(())
}
