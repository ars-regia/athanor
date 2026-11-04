//! Orca's keyboard monitor (doc_own_stack.md OS4, doc_accessibility.md AX4):
//! `org.freedesktop.a11y.KeyboardMonitor` at `/org/freedesktop/a11y/Manager`, under the name
//! `org.freedesktop.a11y.Manager` on the session bus, the interface libatspi's `AtspiDevice`
//! calls and cosmic-comp serves. A client watches the keyboard, grabs it whole, or grabs single
//! keys; a watcher is sent every key but the protected ones as a `KeyEvent` signal addressed to
//! it alone, and a grabbed press, with its release, reaches no other client.
//!
//! Admission (AX4): a call counts only from the owner of `org.gnome.Orca.KeyboardMonitor` whose
//! process lies in `orca.service`'s cgroup and runs an executable under `/usr` that is the very
//! file at that path (`identity.rs`'s unit and file checks). The process is read through the
//! pidfd the bus's `GetConnectionCredentials` returns as `ProcessFD`; from a bus without it, through
//! a pidfd opened on its `ProcessID`, whose connection must still name that pid once it is read
//! (AX5's reuse check). Orca is a Python script, so its executable is the interpreter: the unit
//! is what tells Orca from another program under `/usr`. Every call is admitted anew, and every
//! change of the name's owner, or of a connection, admits each admitted client again and
//! forgets those that fail. A refused call changes nothing and is answered `AccessDenied`; the
//! journal names it, at most once a minute.
//!
//! Threads and connections: the interface is served on one connection, whose calls zbus hands
//! it one at a time, in the order the bus sent them; each call waits for its admission and is
//! answered after it. `keyboard-monitor` admits the calls, and the admitted clients again after a
//! change of names, one at a time, and hands the event loop an `Update` through a channel. It asks
//! the bus on a second connection, which also carries the changes of names that
//! `keyboard-monitor-names` reads. zbus stops reading a connection while one of its queues is
//! full: the calls waiting on the first never stand between the admission and the bus's answers,
//! and the reader of names never waits on the admission. `keyboard-monitor-signals` sends the
//! signals the loop queues (`Monitor::observe`). The loop never waits on the bus.

use crate::identity::{self, Process};
use crate::state::State;
use crate::Error;
use smithay::input::keyboard::{Keysym, KeysymHandle, ModifiersState};
use smithay::reexports::calloop::{channel, LoopHandle};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::os::fd::AsFd;
use std::path::{Component, Path};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::connection::Builder;
use zbus::blocking::Connection;
use zbus::fdo::ConnectionCredentials;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::object_server::SignalEmitter;

pub(crate) const PATH: &str = "/org/freedesktop/a11y/Manager";
pub(crate) const NAME: &str = "org.freedesktop.a11y.Manager";
pub(crate) const INTERFACE: &str = "org.freedesktop.a11y.KeyboardMonitor";
/// The name Orca 49 claims for the monitor (`Atspi.Device.new_full("org.gnome.Orca")`).
pub(crate) const ORCA: &str = "org.gnome.Orca.KeyboardMonitor";
/// The unit Orca runs as (AX3).
const ORCA_UNIT: &str = "orca.service";
/// In a key grab's modifiers, the bit of the first virtual modifier `SetKeyGrabs` names; the
/// bits below are the real modifiers (at-spi2-core's `ATSPI_DEVICE_A11Y_MANAGER_VIRTUAL_MOD_START`).
const VIRTUAL_MOD_START: u32 = 15;
/// The signals the queue holds; a full queue drops a key's signal, with a warning.
const QUEUE: usize = 256;
/// The jobs the admission's queue holds: the one call zbus hands the interface at a time, and
/// the one wake-up after changes of names that `wake` lets wait.
const JOBS: usize = 4;
/// How long a warning that repeats is held back.
const HUSH: Duration = Duration::from_secs(60);
/// xkb's Lock modifier, which Caps Lock sets among the modifiers held while it is down.
const LOCK: u32 = 1 << 1;

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

    /// Whether the grab is the key `sym`, with exactly `mods` held and the virtual modifiers
    /// `active`. A capital letter matches its small one too: Orca grabs a key without Shift by
    /// its small letter (`keybindings.py`, `create_key_definitions`), and Caps Lock, locked or
    /// held as Orca's modifier, capitalizes it. Shift+H stays no grab of H alone, since the
    /// modifiers must match, all but Lock: Caps Lock, held as Orca's modifier, sets it, and Orca
    /// grabs no key with it. A key is read under the keyboard's state, not at its first level,
    /// so that a keypad digit under Num Lock is not the KP_Up Orca grabs.
    fn takes(&self, sym: Keysym, mods: u32, active: &BTreeSet<u32>) -> bool {
        (self.keysym == sym.raw() || self.keysym == uncapped(sym).raw())
            && self.mods == mods & !LOCK
            && self.virtual_mods == *active
    }
}

/// The small letter of a capital one, or the keysym itself.
fn uncapped(sym: Keysym) -> Keysym {
    let Some(letter) = sym.key_char() else {
        return sym;
    };
    let mut small = letter.to_lowercase();
    match (small.next(), small.next()) {
        (Some(small), None) if small != letter => Keysym::from_char(small),
        _ => sym,
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

/// What `Monitor::takes` took a press as.
#[derive(Debug, PartialEq)]
pub(crate) enum Taken {
    /// One of Orca's virtual modifiers, which changes no modifier of the keyboard.
    Modifier,
    /// A key Orca grabs, alone or with the whole keyboard.
    Key,
}

/// A virtual modifier held: its keysym, and the keyboard's locked modifiers before its press.
#[derive(Debug)]
struct Held {
    keysym: u32,
    locked: u32,
}

/// A warning that repeats, said at most once every `HUSH`: a client that floods the bus, or a
/// signal thread that stalls, does not flood the journal.
#[derive(Debug, Default)]
struct Hush {
    said: Option<Instant>,
    held: u64,
}

impl Hush {
    /// Whether to say the warning now, and how many were held back since it was last said.
    fn now(&mut self) -> Option<u64> {
        let now = Instant::now();
        if self
            .said
            .is_some_and(|said| now.duration_since(said) < HUSH)
        {
            self.held += 1;
            return None;
        }
        self.said = Some(now);
        Some(std::mem::take(&mut self.held))
    }
}

/// The event loop's side of the monitor: the admitted clients and the keys held.
#[derive(Debug, Default)]
pub(crate) struct Monitor {
    clients: HashMap<String, Client>,
    /// The virtual modifiers held, by xkb keycode: what is let go is the key that was pressed,
    /// whatever keysym the keyboard's state reads for it then.
    active: BTreeMap<u32, Held>,
    /// The protected keys held, by xkb keycode: their release is sent to no client either.
    withheld: BTreeSet<u32>,
    full: Hush,
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
        self.active.retain(|_, held| {
            clients
                .values()
                .any(|c| c.virtual_mods.contains(&held.keysym))
        });
    }

    /// The keysyms of the virtual modifiers held.
    fn active(&self) -> BTreeSet<u32> {
        self.active.values().map(|held| held.keysym).collect()
    }

    /// Queues the key for each client that watches the keyboard, grabs it whole, or grabs this
    /// key: every key, bound or not, aimed at any surface, the lock's included, but a
    /// `protected` press, with its release. A protected key fires an action the user's way out
    /// and the trusted path keep (`Action::never_inhibited`): as OS4 says of the reveal key, no
    /// client is ever sent it, Orca included (the maintainer's decision of 2026-10-30, #863).
    pub(crate) fn observe(
        &mut self,
        modifiers: &ModifiersState,
        handle: &KeysymHandle<'_>,
        released: bool,
        protected: bool,
    ) {
        let code = handle.raw_code().raw();
        if released {
            if self.withheld.remove(&code) {
                return;
            }
        } else if protected {
            self.withheld.insert(code);
            return;
        }
        let Some(events) = &self.events else {
            return;
        };
        let keysym = handle.modified_sym();
        let mods = modifiers.serialized.depressed;
        let active = self.active();
        for (name, client) in &self.clients {
            let grabs = |grab: &KeyGrab| grab.takes(keysym, mods, &active);
            if !(client.watched || client.grabbed || client.key_grabs.iter().any(grabs)) {
                continue;
            }
            let event = KeyEvent {
                destination: name.clone(),
                released,
                state: mods,
                keysym: keysym.raw(),
                unichar: keysym.key_char().map_or(0, u32::from),
                keycode: u16::try_from(code).unwrap_or_default(),
            };
            if let Err(error) = events.try_send(event) {
                if let Some(held) = self.full.now() {
                    tracing::warn!(%error, held, "a key is not sent to Orca");
                }
            }
        }
    }

    /// Whether a press is kept from the focused client, and as what: a virtual modifier, which
    /// becomes active and is told the keyboard's `locked` modifiers before its press; a key a
    /// client grabs; or any key while a client grabs the keyboard. The caller asks only when no
    /// binding took the key and the session is not locked.
    pub(crate) fn takes(
        &mut self,
        modifiers: &ModifiersState,
        handle: &KeysymHandle<'_>,
        locked: u32,
    ) -> Option<Taken> {
        let keysym = handle.modified_sym();
        if self
            .clients
            .values()
            .any(|c| c.virtual_mods.contains(&keysym.raw()))
        {
            let held = Held {
                keysym: keysym.raw(),
                locked,
            };
            self.active.insert(handle.raw_code().raw(), held);
            return Some(Taken::Modifier);
        }
        let mods = modifiers.serialized.depressed;
        let active = self.active();
        let grabs = |grab: &KeyGrab| grab.takes(keysym, mods, &active);
        self.clients
            .values()
            .any(|c| c.grabbed || c.key_grabs.iter().any(grabs))
            .then_some(Taken::Key)
    }

    /// A key let go: if it was an active virtual modifier, which is no longer, the keyboard's
    /// locked modifiers before its press.
    pub(crate) fn released(&mut self, handle: &KeysymHandle<'_>) -> Option<u32> {
        self.active
            .remove(&handle.raw_code().raw())
            .map(|held| held.locked)
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

/// The admission's answer to one call, shared by its two ends.
#[derive(Default)]
struct Slot {
    admitted: bool,
    answered: bool,
    waker: Option<Waker>,
}

fn slot(slot: &Mutex<Slot>) -> MutexGuard<'_, Slot> {
    // A slot holds three plain fields, which no panic leaves half written.
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The admission's end of a call: `admit` it, or drop it, which refuses it.
struct Answer(Arc<Mutex<Slot>>);

impl Answer {
    fn admit(self) {
        slot(&self.0).admitted = true;
    }
}

impl Drop for Answer {
    fn drop(&mut self) {
        let mut slot = slot(&self.0);
        slot.answered = true;
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// The call's end: whether the call was admitted, once it is answered.
struct Answered(Arc<Mutex<Slot>>);

impl Future for Answered {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<bool> {
        let mut slot = slot(&self.0);
        if slot.answered {
            return Poll::Ready(slot.admitted);
        }
        slot.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

fn answer() -> (Answer, Answered) {
    let shared = Arc::new(Mutex::new(Slot::default()));
    (Answer(shared.clone()), Answered(shared))
}

/// What the `keyboard-monitor` thread admits.
enum Job {
    Call(String, Change, Answer),
    /// Read the flags of `Names`.
    Wake,
}

/// What `keyboard-monitor-names` saw: a change it raises, which the admission lowers before it
/// admits each client again, and the end of the changes; and a wake-up queued, not yet taken.
#[derive(Default)]
struct Names {
    changed: AtomicBool,
    blind: AtomicBool,
    waking: AtomicBool,
}

impl Names {
    /// Wakes the admission, unless it has ended, with one wake-up at most in its queue: a
    /// connection that comes and goes in a loop leaves room for Orca's call. A full queue holds a
    /// job already, after which the admission reads the flags: a wake-up dropped then is not
    /// needed.
    fn wake(&self, jobs: &SyncSender<Job>) -> bool {
        if self.waking.swap(true, Ordering::SeqCst) {
            return true;
        }
        match jobs.try_send(Job::Wake) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.waking.store(false, Ordering::SeqCst);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// The admission took the wake-up: a change after this point queues another.
    fn woken(&self) {
        self.waking.store(false, Ordering::SeqCst);
    }
}

/// The interface, on zbus's executor: each call waits for its admission.
struct Interface {
    jobs: SyncSender<Job>,
}

impl Interface {
    async fn ask(&self, header: &Header<'_>, change: Change) -> zbus::fdo::Result<()> {
        let Some(client) = header.sender() else {
            return Err(zbus::fdo::Error::AccessDenied(
                "a call with no sender".into(),
            ));
        };
        let (answer, answered) = answer();
        if let Err(error) = self
            .jobs
            .try_send(Job::Call(client.to_string(), change, answer))
        {
            return Err(zbus::fdo::Error::LimitsExceeded(format!(
                "the keyboard monitor cannot take the call: {error}"
            )));
        }
        if answered.await {
            Ok(())
        } else {
            Err(zbus::fdo::Error::AccessDenied(format!(
                "the keyboard monitor serves Orca alone: {ORCA}'s owner, in {ORCA_UNIT}"
            )))
        }
    }
}

// One call at a time, in the bus's order: a grab and its release are never applied the wrong
// way round.
#[zbus::interface(name = "org.freedesktop.a11y.KeyboardMonitor", spawn = false)]
impl Interface {
    async fn grab_keyboard(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.ask(&header, Change::Grab(true)).await
    }

    async fn ungrab_keyboard(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.ask(&header, Change::Grab(false)).await
    }

    async fn watch_keyboard(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.ask(&header, Change::Watch(true)).await
    }

    async fn unwatch_keyboard(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.ask(&header, Change::Watch(false)).await
    }

    async fn set_key_grabs(
        &self,
        #[zbus(header)] header: Header<'_>,
        virtual_mods: Vec<u32>,
        keystrokes: Vec<(u32, u32)>,
    ) -> zbus::fdo::Result<()> {
        self.ask(&header, Change::KeyGrabs(virtual_mods, keystrokes))
            .await
    }

    /// A key pressed or released, sent to the one client it is for. Declared so that the
    /// interface's introspection lists it; `keyboard-monitor-signals` sends it through the
    /// blocking connection with the same member and arguments (`emit_signal`), so this
    /// asynchronous form goes unused.
    #[allow(dead_code)]
    #[zbus(signal)]
    async fn key_event(
        emitter: &SignalEmitter<'_>,
        released: bool,
        state: u32,
        keysym: u32,
        unichar: u32,
        keycode: u16,
    ) -> zbus::Result<()>;
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
            let send = |update: Update| {
                updates
                    .send(update)
                    .map_err(|_| Error::from("the event loop has ended"))
            };
            let served = Builder::session().and_then(|served| {
                Connection::session().map(|asking| (served, asking))
            });
            let ended = match served {
                Ok((served, asking)) => serve(served, &asking, &identity::Proc, &send, queued),
                Err(error) => Err(error.into()),
            };
            if let Err(error) = ended {
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

/// Serves the interface on the connection `served` builds, admits its calls, asking the bus on
/// `asking`, and sends the keys of `queued`, until the compositor ends or the changes of names
/// can no longer be read: then every client is forgotten and the interface withdrawn.
pub(crate) fn serve(
    served: Builder<'_>,
    asking: &Connection,
    process: &dyn Process,
    updates: &dyn Fn(Update) -> Result<(), Error>,
    queued: Receiver<KeyEvent>,
) -> Result<(), Error> {
    let bus = DBusProxy::new(asking)?;
    // Read from before the first call can be admitted, so that no change is missed.
    let renames = bus.receive_name_owner_changed()?;
    let names = Arc::new(Names::default());
    let (jobs, admissions) = sync_channel(JOBS);
    let (watcher, seen) = (jobs.clone(), names.clone());
    std::thread::Builder::new()
        .name("keyboard-monitor-names".into())
        .spawn(move || {
            for rename in renames {
                let args = match rename.args() {
                    Ok(args) => args,
                    Err(error) => {
                        tracing::error!(%error, "a change of a name on the bus cannot be read");
                        break;
                    }
                };
                // Orca's name or a connection came or went; another well-known name is no
                // client of the monitor.
                let name = args.name();
                if matches!(name, BusName::Unique(_)) || name.as_str() == ORCA {
                    seen.changed.store(true, Ordering::SeqCst);
                    if !seen.wake(&watcher) {
                        return;
                    }
                }
            }
            seen.blind.store(true, Ordering::SeqCst);
            if !seen.wake(&watcher) {
                tracing::debug!("the keyboard monitor ended before its reader of names");
            }
        })?;
    // The interface is served from the connection's start: its object server is set up in
    // zbus's runtime, which `blocking::Connection::object_server` called first, outside it,
    // is not.
    let served = served.serve_at(PATH, Interface { jobs })?.build()?;
    let emitter = served.clone();
    std::thread::Builder::new()
        .name("keyboard-monitor-signals".into())
        .spawn(move || {
            let mut failed = Hush::default();
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
                    if let Some(held) = failed.now() {
                        tracing::warn!(%error, held, "a key was not sent to Orca");
                    }
                }
            }
        })?;
    let reply =
        served.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())?;
    if reply != zbus::fdo::RequestNameReply::PrimaryOwner {
        return Err(format!("{NAME} is owned by another process ({reply:?})").into());
    }
    tracing::info!("serving Orca's keyboard monitor");

    let mut admitted: HashSet<String> = HashSet::new();
    let mut refusals = Hush::default();
    loop {
        let Ok(job) = admissions.recv() else {
            return Ok(());
        };
        if matches!(job, Job::Wake) {
            names.woken();
        }
        if names.blind.load(Ordering::SeqCst) {
            break;
        }
        if names.changed.swap(false, Ordering::SeqCst) {
            // The clients are not told: the interface has no signal for it.
            let failed: Vec<String> = admitted
                .iter()
                .filter(|client| match admit(&bus, client, process) {
                    Ok(()) => false,
                    Err(error) => {
                        tracing::warn!(%client, %error, "a keyboard monitor client is forgotten");
                        true
                    }
                })
                .cloned()
                .collect();
            for client in failed {
                admitted.remove(&client);
                updates(Update {
                    client,
                    change: Change::Forget,
                })?;
            }
        }
        let Job::Call(client, change, answer) = job else {
            continue;
        };
        match admit(&bus, &client, process) {
            Ok(()) => {
                admitted.insert(client.clone());
                // The loop has the change before the client has its answer.
                updates(Update { client, change })?;
                answer.admit();
            }
            Err(error) => {
                if let Some(held) = refusals.now() {
                    tracing::warn!(%client, %error, held, "a keyboard monitor call is refused");
                }
                if admitted.remove(&client) {
                    updates(Update {
                        client,
                        change: Change::Forget,
                    })?;
                }
            }
        }
    }
    for client in admitted.drain() {
        updates(Update {
            client,
            change: Change::Forget,
        })?;
    }
    // The calls queued, and those that follow, are refused.
    drop(admissions);
    served.object_server().remove::<Interface, _>(PATH)?;
    served.release_name(NAME)?;
    Err("the changes of names on the bus can no longer be read".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The introspection lists the five methods and the `KeyEvent` signal with its arguments.
    #[test]
    fn introspection_lists_the_signal() {
        let (jobs, _) = sync_channel(1);
        let mut xml = String::new();
        zbus::object_server::Interface::introspect_to_writer(&Interface { jobs }, &mut xml, 0);
        for method in [
            "GrabKeyboard",
            "UngrabKeyboard",
            "WatchKeyboard",
            "UnwatchKeyboard",
            "SetKeyGrabs",
        ] {
            assert!(
                xml.contains(&format!("<method name=\"{method}\"")),
                "{method}: {xml}"
            );
        }
        assert!(xml.contains("<signal name=\"KeyEvent\">"), "{xml}");
        for (name, kind) in [
            ("released", "b"),
            ("state", "u"),
            ("keysym", "u"),
            ("unichar", "u"),
            ("keycode", "q"),
        ] {
            let arg = format!("<arg name=\"{name}\" type=\"{kind}\"/>");
            assert!(xml.contains(&arg), "{arg}: {xml}");
        }
    }

    /// Bit 15 and up of a grab's modifiers name the virtual modifiers, in order; the bits below
    /// are the real modifiers.
    #[test]
    fn a_grab_reads_its_virtual_modifiers() {
        let grab = KeyGrab::new(&[0xff63, 0xffe5], 0x61, (1 << 16) | 4);
        assert_eq!(grab.mods, 4);
        assert_eq!(grab.virtual_mods, BTreeSet::from([0xffe5]));
    }

    /// A grab of a small letter takes its capital with the same modifiers, and nothing else.
    #[test]
    fn a_grab_of_a_letter_takes_its_capital() {
        let none = BTreeSet::new();
        let grab = KeyGrab::new(&[], Keysym::h.raw(), 0);
        assert!(grab.takes(Keysym::H, 0, &none), "Caps Lock's H");
        assert!(grab.takes(Keysym::h, 0, &none));
        assert!(grab.takes(Keysym::H, LOCK, &none), "Caps Lock held");
        assert!(!grab.takes(Keysym::H, 1, &none), "Shift+H is not H alone");
        assert!(!grab.takes(Keysym::j, 0, &none));
        let keypad = KeyGrab::new(&[], Keysym::KP_Up.raw(), 0);
        assert!(
            !keypad.takes(Keysym::KP_8, 0, &none),
            "Num Lock's 8 is not KP_Up"
        );
        assert_eq!(uncapped(Keysym::_8), Keysym::_8);
    }

    /// An answer dropped refuses the call; one admitted admits it.
    #[test]
    fn a_dropped_answer_refuses_the_call() {
        let mut context = Context::from_waker(Waker::noop());
        let (refused, mut answered) = answer();
        assert_eq!(Pin::new(&mut answered).poll(&mut context), Poll::Pending);
        drop(refused);
        assert_eq!(
            Pin::new(&mut answered).poll(&mut context),
            Poll::Ready(false)
        );
        let (admitted, mut answered) = answer();
        admitted.admit();
        assert_eq!(
            Pin::new(&mut answered).poll(&mut context),
            Poll::Ready(true)
        );
    }

    /// Changes of names in a loop queue one wake-up, and Orca's call still finds room; taken,
    /// the wake-up lets the next change queue another.
    #[test]
    fn changes_of_names_leave_room_for_a_call() {
        let (jobs, admissions) = sync_channel(JOBS);
        let names = Names::default();
        for _ in 0..100 {
            assert!(names.wake(&jobs));
        }
        let (answer, _answered) = answer();
        let call = jobs.try_send(Job::Call(ORCA.into(), Change::Watch(true), answer));
        assert!(call.is_ok(), "the call is queued");
        assert!(matches!(admissions.try_recv(), Ok(Job::Wake)));
        assert!(matches!(admissions.try_recv(), Ok(Job::Call(..))));
        assert!(admissions.try_recv().is_err(), "one wake-up, one call");
        names.woken();
        assert!(names.wake(&jobs));
        assert!(matches!(admissions.try_recv(), Ok(Job::Wake)));
        names.woken();
        drop(admissions);
        assert!(!names.wake(&jobs), "the admission has ended");
    }

    /// A repeated warning is said once, then held back and counted.
    #[test]
    fn a_repeated_warning_is_held_back() {
        let mut hush = Hush::default();
        assert_eq!(hush.now(), Some(0));
        assert_eq!(hush.now(), None);
        assert_eq!(hush.now(), None);
        hush.said = Instant::now().checked_sub(HUSH);
        assert_eq!(hush.now(), Some(2));
    }
}
