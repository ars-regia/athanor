//! The EIS socket of the portal backend (doc_own_stack.md decision 33, wave 2c; doc_portal.md
//! PT15): `os.athanor.Eis1` at `/os/athanor/Eis1`, under the name `os.athanor.Eis` on the session
//! bus. `ConnectToEIS(o session_handle, u device_types) -> h` creates a socket pair, keeps one end
//! for the event loop and returns the other; `CloseSession(o session_handle)` ends that session's
//! context and is answered once the loop has shut its end down.
//!
//! Admission: a call counts only from the owner of `org.freedesktop.impl.portal.desktop.athanor`
//! that is the main process of `xdg-desktop-portal-athanor.service`, as the user's systemd tells
//! it from the caller's pidfd (`athanor_unit::bus_caller::admit`: `GetUnitByPIDFD`, the unit's
//! `MainPID`, its one `ExecStart` `/usr/libexec/xdg-desktop-portal-athanor` and its files in the
//! image), never by reading `/proc/<pid>/exe`, which a confined unit cannot read (#1047). Each
//! question, the one for the name's owner included, waits [`ASK`] at most, and one unanswered
//! refuses. Every call is admitted anew; a refused call changes nothing, is answered
//! `AccessDenied`, and the journal names it at most once a minute.
//! A `device_types` of 0, or with a bit other than 1, 2 and 4, is `InvalidArgs` and creates
//! nothing; a second `ConnectToEIS` for a handle that holds a file descriptor is `Failed`, and the
//! handle stays held until `CloseSession` or the revocation, so a portal session has one file
//! descriptor.
//!
//! Revocation: when the backend's name changes owner, which it does when the backend's
//! connection or process ends, every session's context ends. The sessions are kept on the
//! `eis` thread, which admits the calls one at a time: the reader of names only raises a flag,
//! which the thread reads before its next call, so a context opened for a call admitted before
//! the change is closed after it, through the same ordered channel to the loop.
//!
//! Threads and connections, as the keyboard monitor's: the interface is served on one
//! connection, whose calls wait for the `eis` thread; that thread asks the bus on a second
//! connection, which also carries the changes of names `eis-names` reads. The loop never waits
//! on the bus.

use crate::bindings;
use crate::input::{BUTTONS, KEYS};
use crate::keyboard_monitor::{answer, Answer, Hush, Names};
use crate::state::State;
use crate::xkb_config::Settings;
use crate::Error;
use athanor_unit::bus_caller::{self, OnBus, Questions, Service};
use futures_util::StreamExt;
use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Device, Event, InputEvent, InputTime, KeyState,
    PointerButtonEvent, TouchEvent, TouchSlot,
};
use smithay::backend::libei::{
    EiInput, EiInputConnection, EiInputEvent, EiInputSeat, EiRegion, ScrollEvent,
};
use smithay::input::keyboard::{xkb, Keycode, Keysym, ModifiersState, XkbConfig};
use smithay::reexports::calloop::{
    channel, EventSource, LoopHandle, Poll, PostAction, Readiness, RegistrationToken, Token,
    TokenFactory,
};
use smithay::reexports::reis::{self, eis};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};
use std::collections::{HashMap, HashSet};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::time::Duration;
use zbus::connection::Builder;
use zbus::fdo::DBusProxy;
use zbus::message::Header;
use zbus::{Connection, ObjectPath, OwnedFd};

pub(crate) const PATH: &str = "/os/athanor/Eis1";
pub(crate) const NAME: &str = "os.athanor.Eis";
/// The portal backend's name, which only it owns (doc_portal.md PT1).
pub(crate) const BACKEND: &str = "org.freedesktop.impl.portal.desktop.athanor";
/// The backend's unit and executable (doc_portal.md PT15).
pub(crate) const BACKEND_UNIT: &str = "xdg-desktop-portal-athanor.service";
pub(crate) const BACKEND_EXECUTABLE: &str = "/usr/libexec/xdg-desktop-portal-athanor";
/// The jobs the admission's queue holds: the one call zbus hands the interface at a time, and
/// the one wake-up after a change of the backend's name that `Names::wake` lets wait.
const JOBS: usize = 4;

/// The portal backend's service, whose main process alone is admitted.
pub(crate) const BACKEND_SERVICE: Service<'static> = Service {
    unit: BACKEND_UNIT,
    exec: BACKEND_EXECUTABLE,
};
/// How long one question of the admission waits, as athanor-idle's admission of the backend
/// (issue #1035): unanswered, the call is refused.
pub(crate) const ASK: Duration = Duration::from_secs(2);

/// `builder`, whose every method call fails unanswered after [`ASK`]: the `eis` thread asks the
/// bus on such connections alone, the owner of the backend's name included.
pub(crate) fn within_ask(builder: Builder<'_>) -> Builder<'_> {
    builder.method_timeout(ASK)
}

/// Whether `client`, while `owner` owns the backend's name, is the main process of the
/// backend's unit, or why not.
pub(crate) fn check(questions: &impl Questions, client: &str, owner: &str) -> Result<(), Error> {
    if client != owner {
        return Err(format!("{client} does not own {BACKEND}").into());
    }
    zbus::block_on(bus_caller::admit(questions, client, BACKEND_SERVICE)).map_err(Error::from)
}

/// Asks the bus who owns the backend's name, and `check`s `client`.
pub(crate) fn admit(
    bus: &DBusProxy<'_>,
    questions: &impl Questions,
    client: &str,
) -> Result<(), Error> {
    let owner = zbus::block_on(bus.get_name_owner(BACKEND.try_into()?))?;
    check(questions, client, owner.as_str())
}

/// The device types the user granted, with the portal's values: 1 keyboard, 2 pointer, 4 touch
/// screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Types(u32);

impl Types {
    pub(crate) const KEYBOARD: u32 = 1;
    pub(crate) const POINTER: u32 = 2;
    pub(crate) const TOUCH: u32 = 4;

    /// The types of `device_types`, or `None` for 0 and for a bit other than 1, 2 and 4.
    pub(crate) fn parse(device_types: u32) -> Option<Self> {
        let known = Self::KEYBOARD | Self::POINTER | Self::TOUCH;
        (device_types != 0 && device_types & !known == 0).then_some(Self(device_types))
    }

    pub(crate) fn keyboard(self) -> bool {
        self.0 & Self::KEYBOARD != 0
    }

    pub(crate) fn pointer(self) -> bool {
        self.0 & Self::POINTER != 0
    }

    pub(crate) fn touch(self) -> bool {
        self.0 & Self::TOUCH != 0
    }
}

/// What the `eis` thread hands the event loop, in the order it admitted the calls.
pub(crate) enum Update {
    /// Serve `server` as the EIS context of `handle`, with the devices of `types`.
    Open {
        handle: String,
        types: Types,
        server: UnixStream,
    },
    /// End `handle`'s context, then answer.
    Close {
        handle: String,
        done: Answer<zbus::fdo::Result<()>>,
    },
    /// End every context: the backend's name changed owner, or can no longer be watched.
    CloseAll,
}

/// What the `eis` thread admits.
enum Job {
    Connect {
        client: String,
        handle: String,
        device_types: u32,
        answer: Answer<zbus::fdo::Result<OwnedFd>>,
    },
    Close {
        client: String,
        handle: String,
        answer: Answer<zbus::fdo::Result<()>>,
    },
    /// Read the flags of `Names`.
    Wake,
}

/// The interface, on zbus's executor: each call waits for the `eis` thread.
struct Interface {
    jobs: SyncSender<Job>,
}

impl Interface {
    async fn ask<T>(
        &self,
        header: &Header<'_>,
        job: impl FnOnce(String, Answer<zbus::fdo::Result<T>>) -> Job,
    ) -> zbus::fdo::Result<T> {
        let Some(client) = header.sender() else {
            return Err(zbus::fdo::Error::AccessDenied(
                "a call with no sender".into(),
            ));
        };
        let (answer, answered) = answer();
        if let Err(error) = self.jobs.try_send(job(client.to_string(), answer)) {
            return Err(zbus::fdo::Error::LimitsExceeded(format!(
                "the EIS service cannot take the call: {error}"
            )));
        }
        answered.await.unwrap_or_else(|| {
            Err(zbus::fdo::Error::Failed(
                "the EIS service has stopped".into(),
            ))
        })
    }
}

// One call at a time, in the bus's order: a session is never closed before it is opened.
#[zbus::interface(name = "os.athanor.Eis1", spawn = false)]
impl Interface {
    #[zbus(name = "ConnectToEIS")]
    async fn connect_to_eis(
        &self,
        #[zbus(header)] header: Header<'_>,
        session_handle: ObjectPath<'_>,
        device_types: u32,
    ) -> zbus::fdo::Result<OwnedFd> {
        let handle = session_handle.to_string();
        self.ask(&header, |client, answer| Job::Connect {
            client,
            handle,
            device_types,
            answer,
        })
        .await
    }

    async fn close_session(
        &self,
        #[zbus(header)] header: Header<'_>,
        session_handle: ObjectPath<'_>,
    ) -> zbus::fdo::Result<()> {
        let handle = session_handle.to_string();
        self.ask(&header, |client, answer| Job::Close {
            client,
            handle,
            answer,
        })
        .await
    }
}

/// Starts the service. The bus is reached on the `eis` thread; when it cannot be, or the name
/// is taken, the journal says so once and the compositor runs without EIS.
pub(crate) fn start(handle: &LoopHandle<'static, State>) {
    let (updates, received) = channel::channel();
    let inserted = handle.insert_source(received, |event, _, state| {
        if let channel::Event::Msg(update) = event {
            state.eis_apply(update);
        }
    });
    if let Err(error) = inserted {
        tracing::error!(%error, "the EIS service's channel was not added to the loop");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("eis".into())
        .spawn(move || {
            let send = |update: Update| {
                updates
                    .send(update)
                    .map_err(|_| Error::from("the event loop has ended"))
            };
            // The admission asks on connections that serve nothing and subscribe to nothing
            // (`bus_caller::OnBus`), and the owner of the backend's name on `asking`: each
            // question within `ASK`.
            let connected = zbus::block_on(async {
                let asking = within_ask(Builder::session()).build().await?;
                let query = within_ask(Builder::session()).build().await?;
                let system = within_ask(bus_caller::system_bus()).build().await?;
                let bus = DBusProxy::new(&query).await?;
                Ok::<_, zbus::Error>((asking, query, system, bus))
            });
            let ended = match connected {
                Ok((asking, query, system, bus)) => {
                    let questions = OnBus {
                        bus: &bus,
                        session: &query,
                        system: &system,
                    };
                    serve(Builder::session(), &asking, &questions, &send)
                }
                Err(error) => Err(error.into()),
            };
            if let Err(error) = ended {
                tracing::error!(%error, "the EIS service has stopped; remote desktop sessions get no input");
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "the EIS service did not start");
    }
}

/// Serves the interface on the connection `served` builds and admits its calls, asking the bus
/// on `asking`, until the compositor ends or the backend's name can no longer be watched: then
/// every context is ended and the interface withdrawn.
pub(crate) fn serve(
    served: Builder<'_>,
    asking: &Connection,
    questions: &impl Questions,
    updates: &dyn Fn(Update) -> Result<(), Error>,
) -> Result<(), Error> {
    let bus = zbus::block_on(DBusProxy::new(asking))?;
    // Read from before the first call can be admitted, so that no change is missed.
    let mut renames = zbus::block_on(bus.receive_name_owner_changed_with_args(&[(0, BACKEND)]))?;
    let names = Arc::new(Names::default());
    let (jobs, admissions) = sync_channel(JOBS);
    let (watcher, seen) = (jobs.clone(), names.clone());
    std::thread::Builder::new()
        .name("eis-names".into())
        .spawn(move || {
            zbus::block_on(async {
                while let Some(rename) = renames.next().await {
                    match rename.args() {
                        Ok(args) if args.name().as_str() == BACKEND => {
                            seen.changed.store(true, Ordering::SeqCst);
                            if !seen.wake(&watcher, Job::Wake) {
                                return;
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::error!(%error, "a change of a name on the bus cannot be read");
                            break;
                        }
                    }
                }
                seen.blind.store(true, Ordering::SeqCst);
                if !seen.wake(&watcher, Job::Wake) {
                    tracing::debug!("the EIS service ended before its reader of names");
                }
            })
        })?;
    // The interface is served from the connection's start: the builder sets up its object
    // server before the connection reads a message.
    let served = zbus::block_on(served.serve_at(PATH, Interface { jobs }).build())?;
    let reply = zbus::block_on(
        served.request_name_with_flags(NAME, zbus::fdo::RequestNameFlags::DO_NOT_QUEUE),
    )?;
    if reply != zbus::fdo::RequestNameReply::PrimaryOwner {
        return Err(format!("{NAME} is owned by another process ({reply:?})").into());
    }
    tracing::info!("serving the EIS socket of the portal backend");

    // The sessions that hold a file descriptor.
    let mut open: HashSet<String> = HashSet::new();
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
        if names.changed.swap(false, Ordering::SeqCst) && !open.is_empty() {
            tracing::info!(
                sessions = open.len(),
                "{BACKEND} changed owner: its EIS contexts end"
            );
            open.clear();
            updates(Update::CloseAll)?;
        }
        let (client, refused) = match job {
            Job::Wake => continue,
            Job::Connect {
                client,
                handle,
                device_types,
                answer,
            } => match admit(&bus, questions, &client) {
                Ok(()) => {
                    answer.give(connect(&mut open, handle, device_types, updates)?);
                    continue;
                }
                Err(error) => {
                    answer.give(Err(denied()));
                    (client, error)
                }
            },
            Job::Close {
                client,
                handle,
                answer,
            } => match admit(&bus, questions, &client) {
                Ok(()) => {
                    // The loop answers once the context is gone; an unknown handle is closed.
                    open.remove(&handle);
                    updates(Update::Close {
                        handle,
                        done: answer,
                    })?;
                    continue;
                }
                Err(error) => {
                    answer.give(Err(denied()));
                    (client, error)
                }
            },
        };
        if let Some(held) = refusals.now() {
            tracing::warn!(%client, error = %refused, held, "an EIS call is refused");
        }
    }
    if !open.is_empty() {
        updates(Update::CloseAll)?;
    }
    // The calls queued, and those that follow, are refused.
    drop(admissions);
    zbus::block_on(served.object_server().remove::<Interface, _>(PATH))?;
    zbus::block_on(served.release_name(NAME))?;
    Err("the changes of the backend's name on the bus can no longer be read".into())
}

fn denied() -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied(format!(
        "the EIS socket serves the portal backend alone: {BACKEND}'s owner, in {BACKEND_UNIT}"
    ))
}

/// An admitted `ConnectToEIS`: the client's end of a new socket pair, whose other end the loop
/// is handed, or why not. Only a closed loop is an error of the service.
fn connect(
    open: &mut HashSet<String>,
    handle: String,
    device_types: u32,
    updates: &dyn Fn(Update) -> Result<(), Error>,
) -> Result<zbus::fdo::Result<OwnedFd>, Error> {
    let Some(types) = Types::parse(device_types) else {
        return Ok(Err(zbus::fdo::Error::InvalidArgs(format!(
            "device types {device_types:#x}: 1 keyboard, 2 pointer and 4 touch screen, at least one"
        ))));
    };
    if open.contains(&handle) {
        return Ok(Err(zbus::fdo::Error::Failed(format!(
            "{handle} holds a file descriptor already"
        ))));
    }
    // Both ends close on exec: std opens them with `SOCK_CLOEXEC`.
    let (server, client) = match UnixStream::pair() {
        Ok(pair) => pair,
        Err(error) => {
            return Ok(Err(zbus::fdo::Error::Failed(format!(
                "no socket pair: {error}"
            ))))
        }
    };
    open.insert(handle.clone());
    updates(Update::Open {
        handle,
        types,
        server,
    })?;
    Ok(Ok(OwnedFd::from(std::os::fd::OwnedFd::from(client))))
}

/// One context's source: Smithay's EIS server, which tells the loop when it removes itself, so
/// that the loop drops what it holds for the context. A context that fails ends, as one that
/// disconnects does: it never stops the loop.
struct Served {
    input: EiInput,
    id: u64,
    ended: channel::Sender<u64>,
}

impl EventSource for Served {
    type Event = EiInputEvent;
    type Metadata = EiInputConnection;
    type Ret = ();
    type Error = std::io::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(EiInputEvent, &mut EiInputConnection),
    {
        let action = match self.input.process_events(readiness, token, callback) {
            Ok(action) => action,
            Err(error) => {
                tracing::warn!(%error, "an EIS context failed: it ends");
                PostAction::Remove
            }
        };
        // A receiver context is removed without a word to the callback (Smithay's `EiInput`).
        if matches!(action, PostAction::Remove) && self.ended.send(self.id).is_err() {
            tracing::debug!("an EIS context ended after the event loop");
        }
        Ok(action)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        factory: &mut TokenFactory,
    ) -> Result<(), smithay::reexports::calloop::Error> {
        self.input.register(poll, factory)
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        factory: &mut TokenFactory,
    ) -> Result<(), smithay::reexports::calloop::Error> {
        self.input.reregister(poll, factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> Result<(), smithay::reexports::calloop::Error> {
        self.input.unregister(poll)
    }
}

/// One session's EIS context, on the loop.
struct Context {
    handle: String,
    types: Types,
    token: RegistrationToken,
    /// The server end, kept to shut it down when the session ends: the application's end then
    /// reads end of stream, even while Smithay still holds its own copy.
    socket: UnixStream,
    /// The seat, once the client has finished its handshake.
    seat: Option<EiInputSeat>,
    /// The keyboard's own state, when the keyboard was granted.
    keys: Option<Keys>,
    /// The buttons held, by device: Smithay's device id is unique within one connection only,
    /// so a device is named `eis:<context>:<id>` (`device_key`).
    buttons: HashSet<(String, u32)>,
    /// The touches held, by device and the device's slot.
    touches: HashSet<(String, TouchSlot)>,
    /// The absolute devices that moved the pointer, whose last position the seat keeps.
    absolute: HashSet<String>,
}

/// A context's keyboard: its own keymap, the seat's `xkb_config` compiled again, and its own
/// xkb state, apart from the seat's physical state (`State::eis_key`).
struct Keys {
    keymap: xkb::Keymap,
    state: xkb::State,
    /// The xkb codes pressed and sent to a client.
    held: HashSet<u32>,
    /// The xkb codes whose press was dropped, and whose release is dropped too.
    dropped: HashSet<u32>,
}

impl Keys {
    /// The keyboard of context `handle`, logged when the keymap does not compile: every key the
    /// context sends is then dropped, as with no keyboard.
    fn of(handle: &str, settings: Option<&Settings>) -> Option<Self> {
        let keys = Self::new(settings);
        if keys.is_none() {
            tracing::warn!(%handle, "an EIS keyboard's keymap did not compile; its keys are dropped");
        }
        keys
    }

    fn new(settings: Option<&Settings>) -> Option<Self> {
        let config = xkb_config(settings);
        let keymap = xkb::Keymap::new_from_names(
            &xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
            config.rules,
            config.model,
            config.layout,
            config.variant,
            config.options,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )?;
        Some(Self {
            state: xkb::State::new(&keymap),
            keymap,
            held: HashSet::new(),
            dropped: HashSet::new(),
        })
    }

    fn modifiers(&self) -> ModifiersState {
        let mut modifiers = ModifiersState::default();
        modifiers.update_with(&self.state);
        modifiers
    }

    /// `[depressed, latched, locked, group]`, as `set_modifier_mask` takes them.
    fn mask(&self) -> [u32; 4] {
        [
            self.state.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            self.state.serialize_mods(xkb::STATE_MODS_LATCHED),
            self.state.serialize_mods(xkb::STATE_MODS_LOCKED),
            self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
        ]
    }

    /// The first-level keysyms of `code` in the active layout, then in the others, as the
    /// keyboard aids read a hardware key (`aids.rs`).
    fn raw_syms(&self, code: Keycode) -> Vec<Keysym> {
        let active = self.state.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        let others = (0..self.keymap.num_layouts()).filter(|layout| *layout != active);
        std::iter::once(active)
            .chain(others)
            .flat_map(|layout| self.keymap.key_get_syms_by_level(code, layout, 0).to_vec())
            .collect()
    }
}

/// Whether injected input is delivered: neither from the lock request to the unlock nor while
/// the session is paused on another virtual terminal, as `drops_virtual_input` has it for the
/// virtual keyboard. An event sent then is dropped, not queued.
pub(crate) fn delivers(locked: bool, paused: bool) -> bool {
    !locked && !paused
}

/// Whether `surface` is the portal backend's, named by its unit or its executable as the
/// admission names it: no injected input reaches the consent dialog. It fails closed: a client
/// outside a security context whose identity could not be read, or a surface whose client is
/// gone, counts as the backend's; a confined client never is.
pub(crate) fn backend_surface(surface: &WlSurface) -> bool {
    surface
        .client()
        .is_none_or(|client| match crate::state::identity_of(&client) {
            Some(identity) => {
                identity.unit.as_deref() == Some(BACKEND_UNIT)
                    || identity.executable == Path::new(BACKEND_EXECUTABLE)
            }
            None => crate::state::unidentified(&client),
        })
}

/// A device of context `id`, apart from every other context's.
fn device_key(id: u64, device: &impl Device) -> String {
    format!("eis:{id}:{}", device.id())
}

/// A point of an absolute event, if finite.
fn finite(x: f64, y: f64) -> Option<Point<f64, Logical>> {
    (x.is_finite() && y.is_finite()).then(|| Point::from((x, y)))
}

/// The event loop's side: the contexts, by an id of the loop's own, and the keymap their
/// keyboards are given.
#[derive(Default)]
pub(crate) struct Eis {
    contexts: HashMap<u64, Context>,
    next: u64,
    /// Where a context's source says it removed itself; added to the loop at the first context.
    ended: Option<channel::Sender<u64>>,
    /// The seat's `xkb_config`, once a file was applied; the default before.
    settings: Option<Settings>,
    /// Set while an injected motion, button or scroll is applied: `pointer_target` then gives
    /// no surface of the portal backend the pointer.
    injecting: bool,
    /// How many presses of each button the seat holds, the hardware's and the contexts'.
    buttons: HashMap<u32, usize>,
}

impl Eis {
    pub(crate) fn injecting(&self) -> bool {
        self.injecting
    }

    /// How many contexts are open.
    #[cfg(test)]
    pub(crate) fn open(&self) -> usize {
        self.contexts.len()
    }

    /// A button reached the seat (`State::button_input`).
    pub(crate) fn seat_button(&mut self, button: u32, state: ButtonState) {
        let held = self.buttons.entry(button).or_default();
        match state {
            ButtonState::Pressed => *held += 1,
            ButtonState::Released => *held = held.saturating_sub(1),
        }
        if *held == 0 {
            self.buttons.remove(&button);
        }
    }
}

/// The keymap of `settings`, or the default `State::new` gives the seat.
fn xkb_config(settings: Option<&Settings>) -> XkbConfig<'_> {
    settings.map_or_else(XkbConfig::default, |settings| XkbConfig {
        rules: &settings.rules,
        model: &settings.model,
        layout: &settings.layout,
        variant: &settings.variant,
        options: settings.options.clone(),
    })
}

const SEAT: &str = "athanor";
const KEYBOARD: &str = "athanor keyboard";

impl State {
    /// Applies what the `eis` thread admitted.
    pub(crate) fn eis_apply(&mut self, update: Update) {
        match update {
            Update::Open {
                handle,
                types,
                server,
            } => self.eis_open(handle, types, server),
            Update::Close { handle, done } => {
                let found = self
                    .eis
                    .contexts
                    .iter()
                    .find(|(_, context)| context.handle == handle)
                    .map(|(id, _)| *id);
                if let Some(id) = found {
                    self.eis_close(id);
                }
                done.give(Ok(()));
            }
            Update::CloseAll => {
                let ids: Vec<u64> = self.eis.contexts.keys().copied().collect();
                for id in ids {
                    self.eis_close(id);
                }
            }
        }
    }

    /// Serves `server` as `handle`'s context. A context that cannot be served is dropped, and
    /// the application's end reads end of stream.
    fn eis_open(&mut self, handle: String, types: Types, server: UnixStream) {
        let opened = server
            .try_clone()
            .and_then(|socket| Ok((socket, eis::Context::new(server)?)));
        let (socket, context) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                tracing::warn!(%error, %handle, "an EIS context cannot be served");
                return;
            }
        };
        let Some(ended) = self.eis_ended() else {
            return;
        };
        let id = self.eis.next;
        self.eis.next += 1;
        let source = Served {
            input: EiInput::new(context),
            id,
            ended,
        };
        let inserted = self
            .loop_handle
            .insert_source(source, move |event, connection, state| {
                state.eis_event(id, event, connection)
            });
        let token = match inserted {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(error = %error.error, %handle, "an EIS context was not added to the loop");
                return;
            }
        };
        tracing::info!(%handle, ?types, "an EIS context is open");
        let keys = types
            .keyboard()
            .then(|| Keys::of(&handle, self.eis.settings.as_ref()))
            .flatten();
        self.eis.contexts.insert(
            id,
            Context {
                handle,
                types,
                token,
                socket,
                seat: None,
                keys,
                buttons: HashSet::new(),
                touches: HashSet::new(),
                absolute: HashSet::new(),
            },
        );
    }

    /// The channel a context's source says it ended through, added to the loop once.
    fn eis_ended(&mut self) -> Option<channel::Sender<u64>> {
        if let Some(ended) = &self.eis.ended {
            return Some(ended.clone());
        }
        let (ended, received) = channel::channel();
        let inserted = self.loop_handle.insert_source(received, |event, _, state| {
            if let channel::Event::Msg(id) = event {
                state.eis_forget(id);
            }
        });
        if let Err(error) = inserted {
            tracing::error!(error = %error.error, "the EIS contexts' channel was not added to the loop");
            return None;
        }
        self.eis.ended = Some(ended.clone());
        Some(ended)
    }

    /// Ends context `id`: its source leaves the loop and its socket is shut down.
    fn eis_close(&mut self, id: u64) {
        if let Some(token) = self.eis_forget(id) {
            self.loop_handle.remove(token);
        }
    }

    /// Drops what the loop holds for context `id` and shuts its socket down: the source's token,
    /// if the context was known.
    fn eis_forget(&mut self, id: u64) -> Option<RegistrationToken> {
        self.eis_release(id);
        let context = self.eis.contexts.remove(&id)?;
        for device in &context.absolute {
            self.absolute_device_gone(device);
        }
        if let Err(error) = context.socket.shutdown(Shutdown::Both) {
            tracing::debug!(%error, handle = %context.handle, "an EIS socket was shut down already");
        }
        tracing::info!(handle = %context.handle, "an EIS context has ended");
        Some(context.token)
    }

    fn eis_event(&mut self, id: u64, event: EiInputEvent, connection: &mut EiInputConnection) {
        match event {
            EiInputEvent::Connected => self.eis_connected(id, connection),
            // The source removes itself and says so (`Served`).
            EiInputEvent::Disconnected => {}
            EiInputEvent::Event(event) => self.eis_input(id, event),
        }
    }

    /// The client finished its handshake as a sender: its seat offers the granted devices
    /// alone, which it may bind.
    fn eis_connected(&mut self, id: u64, connection: &mut EiInputConnection) {
        let regions = self.eis_regions();
        let Some(context) = self.eis.contexts.get_mut(&id) else {
            return;
        };
        // Smithay's seat advertises every capability, text included, since a seat's
        // capabilities cannot be added later; a device is created only for a capability added
        // below and bound by the client. So the client is offered no device of an ungranted
        // type and never a text device, though the seat's capabilities name them.
        let seat = connection.add_seat(SEAT);
        if context.types.keyboard() {
            if let Err(error) = seat.add_keyboard(KEYBOARD, xkb_config(self.eis.settings.as_ref()))
            {
                tracing::warn!(?error, handle = %context.handle, "an EIS keyboard was not added");
            }
        }
        if context.types.pointer() {
            seat.add_pointer("athanor pointer");
            seat.add_pointer_absolute("athanor absolute pointer", &regions);
        }
        if context.types.touch() {
            seat.add_touch("athanor touch", &regions);
        }
        context.seat = Some(seat);
    }

    /// The outputs, in the logical space where absolute motion and touches land.
    fn eis_regions(&self) -> Vec<EiRegion> {
        self.space
            .outputs()
            .filter_map(|output| {
                Some(EiRegion {
                    rect: self.space.output_geometry(output)?,
                    scale: output.current_scale().fractional_scale() as f32,
                    mapping_id: Some(output.name()),
                })
            })
            .collect()
    }

    /// The seat's keymap changed: each context's keyboard is given the new one.
    pub(crate) fn eis_keymap_changed(&mut self, settings: &Settings) {
        self.eis.settings = Some(settings.clone());
        let ids: Vec<u64> = self.eis.contexts.keys().copied().collect();
        for id in ids {
            self.eis_release_keys(id);
        }
        let config = xkb_config(self.eis.settings.as_ref());
        for context in self.eis.contexts.values_mut() {
            if context.types.keyboard() {
                context.keys = Keys::of(&context.handle, self.eis.settings.as_ref());
            }
            let Some(seat) = context.seat.as_ref().filter(|_| context.types.keyboard()) else {
                continue;
            };
            seat.remove_keyboard();
            if let Err(error) = seat.add_keyboard(KEYBOARD, config.clone()) {
                tracing::warn!(?error, handle = %context.handle, "an EIS keyboard was not given the new keymap");
            }
        }
    }

    /// Context `id`'s input. It reaches Wayland clients only: no binding of the compositor,
    /// neither the keyboard monitor nor the keyboard aids, and no surface of the portal backend.
    fn eis_input(&mut self, id: u64, event: InputEvent<EiInput>) {
        match &event {
            InputEvent::DeviceAdded { .. } => return,
            InputEvent::DeviceRemoved { device } => return self.eis_device_gone(id, device),
            _ => {}
        }
        if !delivers(self.lock.is_some(), self.session_paused()) {
            return;
        }
        // As for the hardware's input (`process_input_event`).
        self.activity();
        self.request_redraw();
        match event {
            InputEvent::Keyboard { event } => {
                let pressed = event.state == eis::keyboard::KeyState::Press;
                self.eis_key(id, event.key, pressed, Event::time(&event));
            }
            InputEvent::PointerMotion { event } => {
                let Some(delta) = finite(event.dx.into(), event.dy.into()) else {
                    return;
                };
                self.eis_pointing(|state| {
                    state.pointer_moved_by(delta, delta, Event::time(&event))
                });
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let Some(to) = finite(event.x(), event.y()).filter(|to| self.on_outputs(*to))
                else {
                    return;
                };
                let device = device_key(id, &Event::device(&event));
                let Some(context) = self.eis.contexts.get_mut(&id) else {
                    return;
                };
                context.absolute.insert(device.clone());
                self.eis_pointing(|state| state.pointer_moved_to(device, to, Event::time(&event)));
            }
            InputEvent::PointerButton { event } => {
                let device = device_key(id, &Event::device(&event));
                let time = Event::time(&event);
                self.eis_button(id, device, event.button_code(), event.state(), time);
            }
            InputEvent::PointerAxis { event } => {
                if let ScrollEvent::Delta(delta) = &event {
                    if finite(delta.dx.into(), delta.dy.into()).is_none() {
                        return;
                    }
                }
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                if self.eis_aims_at_backend() {
                    return;
                }
                self.eis_pointing(|state| crate::input::scroll(state, &pointer, &event));
            }
            InputEvent::TouchDown { event } => {
                let device = device_key(id, &Event::device(&event));
                let Some(at) = finite(event.x(), event.y()).filter(|at| self.on_outputs(*at))
                else {
                    return;
                };
                if self
                    .surface_under(at)
                    .is_some_and(|(surface, _)| backend_surface(&surface))
                {
                    return;
                }
                let Some(context) = self.eis.contexts.get_mut(&id) else {
                    return;
                };
                let slot = event.slot();
                if !context.touches.insert((device.clone(), slot)) {
                    return;
                }
                self.touch_down(&device, slot, at, Event::time(&event));
            }
            InputEvent::TouchMotion { event } => {
                let key = (device_key(id, &Event::device(&event)), event.slot());
                let Some(at) = finite(event.x(), event.y()) else {
                    return;
                };
                if self
                    .eis
                    .contexts
                    .get(&id)
                    .is_some_and(|context| context.touches.contains(&key))
                {
                    self.touch_motion(&key.0, key.1, at, Event::time(&event));
                }
            }
            InputEvent::TouchUp { event } => {
                let key = (device_key(id, &Event::device(&event)), event.slot());
                self.eis_touch_up(id, key, Event::time(&event));
            }
            // One touch's cancel, which ends that touch alone: `wl_touch.cancel` would end the
            // user's own touches too.
            InputEvent::TouchCancel { event } => {
                let key = (device_key(id, &Event::device(&event)), event.slot());
                self.eis_touch_up(id, key, Event::time(&event));
            }
            InputEvent::TouchFrame { .. } => self.touch_frame(),
            // Text (keysyms and UTF-8) is never offered, and its events are dropped.
            _ => {}
        }
    }

    /// An injected key, under context `id`'s own keymap and modifiers. A press is dropped, its
    /// release with it, when it is a protected key of decision 30 under the context's own
    /// modifiers, when the seat or another context holds that key, or when the keyboard focus
    /// is the portal backend's.
    fn eis_key(&mut self, id: u64, key: u32, pressed: bool, time: InputTime) {
        // Smithay adds 8 to a code the client chose.
        let Some(raw) = key.checked_add(8).filter(|raw| KEYS.contains(raw)) else {
            return;
        };
        let code = Keycode::new(raw);
        let keyboard = self.seat.get_keyboard();
        let seat_holds = keyboard
            .as_ref()
            .is_some_and(|keyboard| keyboard.pressed_keys().contains(&code));
        let to_backend = keyboard
            .as_ref()
            .and_then(|keyboard| keyboard.current_focus())
            .is_some_and(|surface| backend_surface(&surface));
        let another_holds = self.eis.contexts.iter().any(|(other, context)| {
            *other != id
                && context
                    .keys
                    .as_ref()
                    .is_some_and(|keys| keys.held.contains(&raw))
        });
        let Some(keys) = self
            .eis
            .contexts
            .get_mut(&id)
            .and_then(|context| context.keys.as_mut())
        else {
            return;
        };
        let key_state = if pressed {
            if keys.held.contains(&raw) || keys.dropped.contains(&raw) {
                return;
            }
            let protected =
                bindings::protected(bindings::mask(&keys.modifiers()), &keys.raw_syms(code));
            if protected || seat_holds || another_holds || to_backend {
                keys.dropped.insert(raw);
                return;
            }
            keys.state.update_key(code, xkb::KeyDirection::Down);
            keys.held.insert(raw);
            // A key the client gets cancels a hardware Super alone, as an injected button does.
            self.super_tap = None;
            KeyState::Pressed
        } else {
            if keys.dropped.remove(&raw) || !keys.held.remove(&raw) {
                return;
            }
            // The keys held when the backend took the focus were released before
            // (`eis_focusing`), so a held key is the focus's own and its release always goes.
            keys.state.update_key(code, xkb::KeyDirection::Up);
            KeyState::Released
        };
        let mask = keys.mask();
        self.eis_send_key(code, key_state, mask, time);
    }

    /// Sends the focused client a key with the modifiers `mask`, past the input method's grab,
    /// and then the seat's own modifiers again, as a virtual keyboard's key is sent
    /// (`with_keymap_of`): the seat's xkb state, which reads the hardware's keys, never has the
    /// key, and no binding, monitor or aid sees it.
    fn eis_send_key(
        &mut self,
        code: Keycode,
        key_state: KeyState,
        mask: [u32; 4],
        time: InputTime,
    ) {
        // The context's keymap is the seat's own.
        self.restore_seat_keymap();
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let seat = keyboard.modifier_state().serialized;
        let [depressed, latched, locked, group] = mask;
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(depressed, latched, locked, group)
        });
        let serial = SERIAL_COUNTER.next_serial();
        if key_state == KeyState::Pressed {
            self.grab_serials.press = Some(serial);
        }
        keyboard.input_forward_bypassing_grab(self, code, key_state, serial, time, true);
        keyboard.with_xkb_state(self, |mut context| {
            context.set_modifier_mask(
                seat.depressed,
                seat.latched,
                seat.locked,
                seat.layout_effective,
            )
        });
    }

    /// An injected button: a press aimed at the portal backend is dropped, and a release counts
    /// only for a button its device holds.
    fn eis_button(
        &mut self,
        id: u64,
        device: String,
        button: u32,
        button_state: ButtonState,
        time: InputTime,
    ) {
        if !BUTTONS.contains(&button) {
            return;
        }
        let aims_at_backend = self.eis_aims_at_backend();
        // Held by the hardware or another context: its release would end that hold too.
        let seat_holds = self.eis.buttons.get(&button).is_some_and(|held| *held > 0);
        let Some(context) = self.eis.contexts.get_mut(&id) else {
            return;
        };
        let key = (device, button);
        let counts = match button_state {
            ButtonState::Pressed => !aims_at_backend && !seat_holds && context.buttons.insert(key),
            ButtonState::Released => context.buttons.remove(&key),
        };
        if counts {
            self.eis_pointing(|state| state.button_input(button, button_state, time));
        }
    }

    /// Whether a button or a scroll would reach a surface of the portal backend: the one under
    /// the pointer, or the pointer's focus.
    fn eis_aims_at_backend(&self) -> bool {
        let Some(pointer) = self.seat.get_pointer() else {
            return false;
        };
        let focused = pointer
            .current_focus()
            .is_some_and(|surface| backend_surface(&surface));
        focused
            || self
                .surface_under(pointer.current_location())
                .is_some_and(|(surface, _)| backend_surface(&surface))
    }

    fn eis_touch_up(&mut self, id: u64, key: (String, TouchSlot), time: InputTime) {
        let held = self
            .eis
            .contexts
            .get_mut(&id)
            .is_some_and(|context| context.touches.remove(&key));
        if held {
            self.touch_up(&key.0, key.1, time);
        }
    }

    /// The keyboard focus is about to move to `surface`: if it is the portal backend's, every
    /// context's held keys are released to the focus they were pressed on first, so the
    /// backend's `wl_keyboard.enter` lists none of them and no release reaches it.
    pub(crate) fn eis_focusing(&mut self, surface: Option<&WlSurface>) {
        if !surface.is_some_and(backend_surface) {
            return;
        }
        let ids: Vec<u64> = self.eis.contexts.keys().copied().collect();
        for id in ids {
            self.eis_release_keys(id);
        }
    }

    /// Runs `input` with the portal backend kept from the pointer's focus.
    fn eis_pointing(&mut self, input: impl FnOnce(&mut Self)) {
        self.eis.injecting = true;
        input(self);
        self.eis.injecting = false;
    }

    fn on_outputs(&self, at: Point<f64, Logical>) -> bool {
        self.output_areas()
            .iter()
            .any(|area| area.to_f64().contains(at))
    }

    /// A device of context `id` went: what it held is let go.
    fn eis_device_gone(
        &mut self,
        id: u64,
        device: &<EiInput as smithay::backend::input::InputBackend>::Device,
    ) {
        let key = device_key(id, device);
        if device.has_capability(reis::event::DeviceCapability::Keyboard) {
            self.eis_release_keys(id);
            let settings = self.eis.settings.clone();
            if let Some(context) = self.eis.contexts.get_mut(&id) {
                if context.types.keyboard() {
                    context.keys = Keys::of(&context.handle, settings.as_ref());
                }
            }
        }
        let Some(context) = self.eis.contexts.get_mut(&id) else {
            return;
        };
        let buttons: Vec<u32> = context
            .buttons
            .iter()
            .filter(|(of, _)| *of == key)
            .map(|(_, button)| *button)
            .collect();
        context.buttons.retain(|(of, _)| *of != key);
        let touches: Vec<TouchSlot> = context
            .touches
            .iter()
            .filter(|(of, _)| *of == key)
            .map(|(_, slot)| *slot)
            .collect();
        context.touches.retain(|(of, _)| *of != key);
        if context.absolute.remove(&key) {
            self.absolute_device_gone(&key);
        }
        self.eis_let_go(&buttons, &[], &key, &touches);
    }

    /// Lets go of what context `id` holds: its keys and buttons with a release, its touches
    /// with `wl_touch.up` of their slots, never `wl_touch.cancel`, which would end the user's
    /// own touches too. No other context's input, and none of the seat's, is touched.
    fn eis_release(&mut self, id: u64) {
        self.eis_release_keys(id);
        let Some(context) = self.eis.contexts.get_mut(&id) else {
            return;
        };
        let buttons: Vec<u32> = context.buttons.drain().map(|(_, button)| button).collect();
        let touches: Vec<(String, TouchSlot)> = context.touches.drain().collect();
        self.eis_let_go(&buttons, &touches, "", &[]);
    }

    /// Releases `buttons`, and lifts `touches` and the slots `slots` of `device`, framed.
    fn eis_let_go(
        &mut self,
        buttons: &[u32],
        touches: &[(String, TouchSlot)],
        device: &str,
        slots: &[TouchSlot],
    ) {
        let time = InputTime::now();
        for button in buttons {
            self.eis_pointing(|state| state.button_input(*button, ButtonState::Released, time));
        }
        let lifted = touches
            .iter()
            .map(|(device, slot)| (device.as_str(), *slot))
            .chain(slots.iter().map(|slot| (device, *slot)));
        let mut any = false;
        for (device, slot) in lifted {
            self.touch_up(device, slot, time);
            any = true;
        }
        if any {
            self.touch_frame();
        }
    }

    /// Releases the keys context `id` holds and forgets the presses it dropped.
    fn eis_release_keys(&mut self, id: u64) {
        let Some(keys) = self
            .eis
            .contexts
            .get_mut(&id)
            .and_then(|context| context.keys.as_mut())
        else {
            return;
        };
        keys.dropped.clear();
        let held: Vec<u32> = keys.held.drain().collect();
        // Each release is sent with the modifiers that remain once its key is up.
        let releases: Vec<(Keycode, [u32; 4])> = held
            .into_iter()
            .map(|raw| {
                let code = Keycode::new(raw);
                keys.state.update_key(code, xkb::KeyDirection::Up);
                (code, keys.mask())
            })
            .collect();
        let time = InputTime::now();
        for (code, mask) in releases {
            self.eis_send_key(code, KeyState::Released, mask, time);
        }
    }

    /// The lock was requested, or the session paused: every context lets go of its keys and
    /// buttons with a release, and its touches are cancelled with `wl_touch.cancel`, as the
    /// hardware's are.
    pub(crate) fn eis_suspend(&mut self) {
        let ids: Vec<u64> = self.eis.contexts.keys().copied().collect();
        let mut touched = false;
        for id in ids {
            self.eis_release_keys(id);
            let Some(context) = self.eis.contexts.get_mut(&id) else {
                continue;
            };
            let buttons: Vec<u32> = context.buttons.drain().map(|(_, button)| button).collect();
            touched |= !context.touches.is_empty();
            context.touches.clear();
            self.eis_let_go(&buttons, &[], "", &[]);
        }
        if touched {
            self.cancel_touches();
        }
    }
}
