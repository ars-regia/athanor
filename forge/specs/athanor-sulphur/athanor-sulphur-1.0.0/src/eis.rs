//! The EIS socket of the portal backend (doc_own_stack.md decision 33, wave 2c; doc_portal.md
//! PT15): `os.athanor.Eis1` at `/os/athanor/Eis1`, under the name `os.athanor.Eis` on the session
//! bus. `ConnectToEIS(o session_handle, u device_types) -> h` creates a socket pair, keeps one end
//! for the event loop and returns the other; `CloseSession(o session_handle)` ends that session's
//! context and is answered once the loop has shut its end down.
//!
//! Admission: a call counts only from the owner of `org.freedesktop.impl.portal.desktop.athanor`
//! whose process lies in `xdg-desktop-portal-athanor.service`'s cgroup and runs
//! `/usr/libexec/xdg-desktop-portal-athanor`, the very file at that path, read as the keyboard
//! monitor reads Orca (`keyboard_monitor::Caller`). Every call is admitted anew; a refused call
//! changes nothing, is answered `AccessDenied`, and the journal names it at most once a minute.
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

use crate::identity::{self, Process};
use crate::keyboard_monitor::{answer, Answer, Caller, Hush, Names};
use crate::state::State;
use crate::xkb_config::Settings;
use crate::Error;
use futures_util::StreamExt;
use smithay::backend::libei::{EiInput, EiInputConnection, EiInputEvent, EiInputSeat, EiRegion};
use smithay::input::keyboard::XkbConfig;
use smithay::reexports::calloop::{
    channel, EventSource, LoopHandle, Poll, PostAction, Readiness, RegistrationToken, Token,
    TokenFactory,
};
use smithay::reexports::reis::eis;
use std::collections::{HashMap, HashSet};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
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

/// Whether `executable` is the backend's.
fn backend_executable(executable: &Path) -> bool {
    executable == Path::new(BACKEND_EXECUTABLE)
}

/// The portal backend, for the EIS socket.
pub(crate) const BACKEND_CALLER: Caller = Caller {
    name: BACKEND,
    unit: BACKEND_UNIT,
    executable: backend_executable,
};

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
            let served = zbus::block_on(Connection::session())
                .map(|asking| (Builder::session(), asking));
            let ended = match served {
                Ok((served, asking)) => serve(served, &asking, &identity::Proc, &send),
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
    process: &dyn Process,
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
            } => match BACKEND_CALLER.admit(&bus, &client, process) {
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
            } => match BACKEND_CALLER.admit(&bus, &client, process) {
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
        self.eis.contexts.insert(
            id,
            Context {
                handle,
                types,
                token,
                socket,
                seat: None,
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
        let context = self.eis.contexts.remove(&id)?;
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
            // Delivered by the routing below; nothing reaches a client before it.
            EiInputEvent::Event(_) => {}
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
        let config = xkb_config(self.eis.settings.as_ref());
        for context in self.eis.contexts.values() {
            let Some(seat) = context.seat.as_ref().filter(|_| context.types.keyboard()) else {
                continue;
            };
            seat.remove_keyboard();
            if let Err(error) = seat.add_keyboard(KEYBOARD, config.clone()) {
                tracing::warn!(?error, handle = %context.handle, "an EIS keyboard was not given the new keymap");
            }
        }
    }
}
