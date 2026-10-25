//! The connection: GTK's own `wl_display` (doc_shell.md, P4), or a plain Wayland connection
//! for a surface drawn without GTK, a private event queue on it, and the protocol events
//! turned into our own types.
//!
//! The queue is read from the GLib main loop by a source of its own, libwayland's pattern for
//! a second queue on a shared display. The socket has other readers: GDK's source, the EGL
//! and Vulkan WSI dispatching their own queues while they swap, GDK's roundtrips. Each read
//! also moves the events for our queue into it and can leave the socket empty, so a watch on
//! the descriptor alone would sleep on them. The source looks at the queue before every poll
//! and after it, and waits on the descriptor only while the queue is empty.

use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::fs::File;
use std::io::ErrorKind;
use std::ops::RangeInclusive;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use athanor_layout::placement::Output;
#[cfg(feature = "gtk")]
use gdk4_wayland::prelude::*;
#[cfg(feature = "gtk")]
use gtk4::gdk;
use wayland_client::backend::{ObjectId, WaylandError};
use wayland_client::globals::{registry_queue_init, GlobalList, GlobalListContents};
#[cfg(feature = "gtk")]
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_registry, wl_seat};
use wayland_client::{
    event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1::WpSecurityContextManagerV1,
    wp_security_context_v1::WpSecurityContextV1,
};

use crate::keymap::{self, MAX_KEYMAP};
use crate::model::{
    Accessibility, Change, Event, ScreenFilter, Table, Tiling, Window, WindowId, WindowState,
    Workspace, WorkspaceId,
};
use crate::protocols::a11y::client::cosmic_a11y_manager_v1::{
    self, ActiveState, CosmicA11yManagerV1, Filter,
};
use crate::protocols::keyboard_layout::client::{
    zcosmic_keyboard_layout_manager_v1::ZcosmicKeyboardLayoutManagerV1,
    zcosmic_keyboard_layout_v1::{self, ZcosmicKeyboardLayoutV1},
};
use crate::protocols::toplevel_info::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};
use crate::protocols::toplevel_management::client::zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1;
use crate::protocols::toplevel_state::client::{
    athanor_toplevel_manager_v1::AthanorToplevelManagerV1,
    athanor_toplevel_state_v1::{self, AthanorToplevelStateV1},
};
use crate::protocols::workspace_v2::client::{
    zcosmic_workspace_handle_v2::{self, TilingState, ZcosmicWorkspaceHandleV2},
    zcosmic_workspace_manager_v2::ZcosmicWorkspaceManagerV2,
};

/// The sandbox engine of every context the shell creates (doc_bar.md, BR2).
pub(crate) const ENGINE: &str = "os.athanor.shell";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the display is not a Wayland display")]
    NotWayland,
    #[error("the Wayland connection failed: {0}")]
    Connection(String),
    #[error("the compositor does not offer {0}")]
    Unavailable(&'static str),
    #[error("no such window")]
    NoWindow,
    #[error("no such workspace")]
    NoWorkspace,
    #[error("invalid argument: {0}")]
    InvalidArgument(&'static str),
    #[error("the display was closed")]
    Closed,
    #[error("the window's picture was not taken: {0}")]
    Capture(&'static str),
}

/// Ids are handed out in creation order across every client of the process, so an id is
/// never reused. The protocol's object ids are, which is why they are not ours.
pub(crate) fn fresh() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The shell's view of the compositor. Every call happens on the thread that runs GLib's
/// default main context: GTK's main thread, or the thread of a calloop loop that runs it.
/// One per Wayland connection: athanor-compositor allows a connection one state object of
/// each window, and ends a connection that makes a second (`already_made`).
pub struct Client {
    pub(crate) inner: Rc<Inner>,
}

type Handler = Box<dyn FnMut(&Client, &Event)>;
type OutputsHandler = Box<dyn Fn(Vec<Output>)>;

/// GDK's display, for a client connected through it, and its `closed` handler.
#[cfg(feature = "gtk")]
struct Gdk {
    display: gdk::Display,
    closed_handler: glib::SignalHandlerId,
}

pub(crate) struct Inner {
    #[cfg(feature = "gtk")]
    gdk: RefCell<Option<Gdk>>,
    connection: Connection,
    pub(crate) qh: QueueHandle<State>,
    /// The compositor's globals, for those bound only when a call needs them: the capture
    /// globals, which a client that never captures never binds.
    #[cfg(feature = "gtk")]
    pub(crate) registry: GlobalList,
    queue: RefCell<EventQueue<State>>,
    pub(crate) state: RefCell<State>,
    handler: RefCell<Option<Handler>>,
    delivering: Cell<bool>,
    outputs_handler: RefCell<Option<OutputsHandler>>,
    /// The outputs the outputs handler last received.
    last_outputs: RefCell<Vec<Output>>,
    /// A failure met while looking at the queue outside `pump`, reported by the next `pump`.
    failure: RefCell<Option<Error>>,
    /// Set for good when the display closes or reading from the compositor fails: every
    /// later call returns [`Error::Closed`].
    closed: Cell<bool>,
    source: glib::Source,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Idempotent: the source may have removed itself after a failure or a close.
        self.source.destroy();
        #[cfg(feature = "gtk")]
        if let Some(gdk) = self.gdk.get_mut().take() {
            gdk.display.disconnect(gdk.closed_handler);
        }
    }
}

impl Client {
    /// Binds what the compositor offers and reads the initial state; the capture globals
    /// wait for the first [`Client::capture`]. A global the compositor does not offer is not
    /// an error: its part of the API stays empty, and its actions return
    /// [`Error::Unavailable`].
    #[cfg(feature = "gtk")]
    pub fn connect(display: &gdk::Display) -> Result<Client, Error> {
        let wayland = display
            .downcast_ref::<gdk4_wayland::WaylandDisplay>()
            .ok_or(Error::NotWayland)?;
        let wl_display = wayland.wl_display().ok_or(Error::NotWayland)?;
        let backend = wl_display
            .backend()
            .upgrade()
            .ok_or_else(|| Error::Connection("the display has no live backend".into()))?;
        let client = Self::attach(Connection::from_backend(backend))?;
        let closing = Rc::downgrade(&client.inner);
        let closed_handler = display.connect_closed(move |_, _| {
            if let Some(inner) = closing.upgrade() {
                inner.close();
            }
        });
        client.inner.gdk.replace(Some(Gdk {
            display: display.clone(),
            closed_handler,
        }));
        Ok(client)
    }

    /// As [`Client::connect`], on a Wayland connection the caller made, for a surface drawn
    /// without GTK. The queue is still read from GLib's default main context: a calloop loop
    /// runs that context through `glib_loop::GlibSource`. Such a connection has no
    /// `closed` signal; a failure of the socket ends the reading, as with GDK.
    pub fn connect_wayland(connection: &Connection) -> Result<Client, Error> {
        Self::attach(connection.clone())
    }

    fn attach(connection: Connection) -> Result<Client, Error> {
        let (globals, mut queue) = registry_queue_init::<State>(&connection)
            .map_err(|err| Error::Connection(err.to_string()))?;
        let qh = queue.handle();
        let mut state = State::bind(&globals, &qh);
        // The first roundtrip announces the objects, the second their initial state.
        for _ in 0..2 {
            queue
                .roundtrip(&mut state)
                .map_err(|err| Error::Connection(err.to_string()))?;
        }
        state.events.clear();

        // The source holds a `Weak<Inner>`, which must stay on this thread: refuse a default
        // main context that another thread owns, as glib's own `*_local` sources do.
        let context = glib::MainContext::default();
        let _owner = context
            .acquire()
            .map_err(|err| Error::Connection(err.to_string()))?;
        let fd = connection.backend().poll_fd().as_raw_fd();
        let inner = Rc::new_cyclic(|weak: &std::rc::Weak<Inner>| Inner {
            #[cfg(feature = "gtk")]
            gdk: RefCell::new(None),
            connection,
            qh,
            #[cfg(feature = "gtk")]
            registry: globals,
            queue: RefCell::new(queue),
            state: RefCell::new(state),
            handler: RefCell::new(None),
            delivering: Cell::new(false),
            outputs_handler: RefCell::new(None),
            last_outputs: RefCell::new(Vec::new()),
            failure: RefCell::new(None),
            closed: Cell::new(false),
            source: queue_source::attach(weak.clone(), fd, &context),
        });
        Ok(Client { inner })
    }

    /// The handler receives every change after the compositor finished describing it. It
    /// may call any method of the client, actions included. Connecting again replaces it.
    pub fn connect_events(&self, handler: impl FnMut(&Client, &Event) + 'static) {
        self.inner.handler.replace(Some(Box::new(handler)));
    }

    /// The outputs as `wl_output` describes them, in the order the compositor announced
    /// them, in logical pixels: the current mode, turned by the output's transform and
    /// divided by its integer scale. Under a fractional scale the size is the one of the
    /// next integer scale; its shape, and whether it is sized, are exact.
    pub fn outputs(&self) -> Vec<Output> {
        self.inner.state.borrow().outputs()
    }

    /// The handler receives every output each time one is added or removed, or changes
    /// size, rotation, scale or connector, as `outputs::watch` does for GDK's monitors.
    /// Connecting again replaces it.
    pub fn connect_outputs(&self, handler: impl Fn(Vec<Output>) + 'static) {
        self.inner.last_outputs.replace(self.outputs());
        self.inner.outputs_handler.replace(Some(Box::new(handler)));
    }

    pub fn windows(&self) -> Vec<Window> {
        self.inner
            .state
            .borrow()
            .windows
            .values()
            .cloned()
            .collect()
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        self.inner
            .state
            .borrow()
            .workspaces
            .values()
            .cloned()
            .collect()
    }

    /// The configured layouts in group order; empty when the compositor sends no XKB keymap.
    pub fn keyboard_layouts(&self) -> Vec<String> {
        self.inner.state.borrow().keyboard_layouts.clone()
    }

    pub fn keyboard_group(&self) -> u32 {
        self.inner.state.borrow().keyboard_group
    }

    /// `None` when the compositor offers no accessibility protocol.
    pub fn accessibility(&self) -> Option<Accessibility> {
        let state = self.inner.state.borrow();
        state.globals.a11y.as_ref().map(|_| state.accessibility)
    }

    pub fn activate(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_action(window, Action::Activate)
    }

    pub fn minimize(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_action(window, Action::Minimize)
    }

    pub fn unminimize(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_action(window, Action::Unminimize)
    }

    pub fn close(&self, window: WindowId) -> Result<(), Error> {
        self.toplevel_action(window, Action::Close)
    }

    pub fn set_tiling(&self, workspace: WorkspaceId, tiling: Tiling) -> Result<(), Error> {
        let state = self.inner.live_state()?;
        let manager = state
            .globals
            .workspace_manager
            .as_ref()
            .ok_or(Error::Unavailable("ext_workspace_manager_v1"))?;
        let handles = state
            .workspace_handles
            .get(&workspace)
            .ok_or(Error::NoWorkspace)?;
        let cosmic = handles
            .cosmic
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_workspace_manager_v2"))?;
        cosmic.set_tiling_state(match tiling {
            Tiling::Floating => TilingState::FloatingOnly,
            Tiling::Tiled => TilingState::TilingEnabled,
        });
        manager.commit();
        drop(state);
        self.flush()
    }

    /// The compositor ignores a group beyond the configured layouts.
    pub fn set_keyboard_group(&self, group: u32) -> Result<(), Error> {
        let state = self.inner.live_state()?;
        state
            .keyboard_layout
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_keyboard_layout_manager_v1"))?
            .set_group(group);
        drop(state);
        self.flush()
    }

    pub fn set_magnifier(&self, enabled: bool) -> Result<(), Error> {
        let state = self.inner.live_state()?;
        state.a11y()?.set_magnifier(active_state(enabled));
        drop(state);
        self.flush()
    }

    pub fn set_screen_filter(&self, inverted: bool, filter: ScreenFilter) -> Result<(), Error> {
        let filter = match filter {
            ScreenFilter::None => Filter::Disabled,
            ScreenFilter::Greyscale => Filter::Greyscale,
            ScreenFilter::Protanopia => Filter::DaltonizeProtanopia,
            ScreenFilter::Deuteranopia => Filter::DaltonizeDeuteranopia,
            ScreenFilter::Tritanopia => Filter::DaltonizeTritanopia,
            ScreenFilter::Unknown => {
                return Err(Error::InvalidArgument(
                    "an unknown screen filter cannot be set",
                ))
            }
        };
        let state = self.inner.live_state()?;
        state
            .a11y()?
            .set_screen_filter(active_state(inverted), filter);
        drop(state);
        self.flush()
    }

    /// Creates a security context listening on `listen`, alive until `close` is readable
    /// (wp_security_context_v1). The compositor holds its own copies once this returns.
    pub(crate) fn create_context(
        &self,
        listen: BorrowedFd<'_>,
        close: BorrowedFd<'_>,
        app_id: &str,
        instance_id: &str,
    ) -> Result<(), Error> {
        let state = self.inner.live_state()?;
        let manager = state
            .globals
            .security_context
            .as_ref()
            .ok_or(Error::Unavailable("wp_security_context_manager_v1"))?;
        let context = manager.create_listener(listen, close, &self.inner.qh, ());
        context.set_sandbox_engine(ENGINE.into());
        context.set_app_id(app_id.into());
        context.set_instance_id(instance_id.into());
        context.commit();
        context.destroy();
        drop(state);
        self.flush()
    }

    /// An `xdg_activation_v1` token from the surface that received the last input event,
    /// so the window it starts takes the focus. GDK waits for the token on its own queue;
    /// what it read for ours meanwhile is dispatched here. A client on a plain connection
    /// has none: the token needs the last input event's serial, which the application
    /// that owns the surfaces holds.
    pub fn activation_token(&self, app: Option<&gio::AppInfo>) -> Option<String> {
        if self.inner.closed.get() {
            return None;
        }
        let token = self.inner.startup_notify_id(app);
        if let Err(err) = self.pump() {
            tracing::warn!("reading from the compositor after the activation token failed: {err}");
        }
        token
    }

    /// Dispatches what GDK read for this queue outside the main loop's own reading, such
    /// as during the roundtrip of an activation token. Flushes through `Inner::flush`, so
    /// a full socket is not a failure.
    pub(crate) fn pump(&self) -> Result<(), Error> {
        self.inner.pump()
    }

    /// Whether the client stopped for good: the display closed, or reading from the
    /// compositor failed, after which no event comes. An application on its own
    /// connection (`connect_wayland`) has no other sign of it.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.get()
    }

    /// [`Error::Closed`] once the display closed.
    pub(crate) fn live(&self) -> Result<(), Error> {
        self.inner.live()
    }

    /// The compositor's socket as `WAYLAND_DISPLAY` names it for a client the shell starts:
    /// the path this connection is connected to. A connection inherited through
    /// `WAYLAND_SOCKET` has no path; GDK's name for the display is then the only one, and a
    /// plain connection has none.
    pub(crate) fn display_name(&self) -> Result<String, Error> {
        self.inner.live()?;
        let fd = self
            .inner
            .connection
            .backend()
            .poll_fd()
            .try_clone_to_owned()
            .map_err(|err| Error::Connection(err.to_string()))?;
        let peer = UnixStream::from(fd)
            .peer_addr()
            .map_err(|err| Error::Connection(err.to_string()))?;
        peer.as_pathname()
            .and_then(|path| path.to_str())
            .map(str::to_owned)
            .or_else(|| self.inner.display_name())
            .ok_or_else(|| Error::Connection("the connection has no socket path".into()))
    }

    /// Through cosmic-comp's manager, else athanor-compositor's: no compositor offers both,
    /// so the order never chooses, and the COSMIC path stays as it was (plan D3).
    fn toplevel_action(&self, window: WindowId, action: Action) -> Result<(), Error> {
        let state = self.inner.live_state()?;
        if let Some(manager) = &state.globals.toplevel_manager {
            let toplevel = state.cosmic_toplevel(window)?;
            match action {
                Action::Activate => manager.activate(toplevel, state.seat()?),
                Action::Minimize => manager.set_minimized(toplevel),
                Action::Unminimize => manager.unset_minimized(toplevel),
                Action::Close => manager.close(toplevel),
            }
        } else {
            let manager = state
                .globals
                .athanor_toplevel
                .as_ref()
                .ok_or(Error::Unavailable("athanor_toplevel_manager_v1"))?;
            let toplevel = &state.toplevels.get(&window).ok_or(Error::NoWindow)?.ext;
            match action {
                Action::Activate => manager.activate(toplevel, state.seat()?),
                Action::Minimize => manager.set_minimized(toplevel),
                Action::Unminimize => manager.unset_minimized(toplevel),
                Action::Close => manager.close(toplevel),
            }
        }
        drop(state);
        self.flush()
    }

    pub(crate) fn flush(&self) -> Result<(), Error> {
        self.inner.flush()
    }
}

/// A full socket is not a failure: what could not be written stays in the shared
/// `wl_display` buffer, which GDK flushes on every main-loop iteration and our source before
/// every poll; a read with nothing to read is not one either.
#[derive(Clone, Copy)]
enum Action {
    Activate,
    Minimize,
    Unminimize,
    Close,
}

fn tolerate_would_block(result: Result<(), WaylandError>) -> Result<(), WaylandError> {
    match result {
        Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => Ok(()),
        other => other,
    }
}

impl Inner {
    fn live(&self) -> Result<(), Error> {
        if self.closed.get() {
            Err(Error::Closed)
        } else {
            Ok(())
        }
    }

    /// The state, to send a request through its proxies: refused once the display closed.
    fn live_state(&self) -> Result<Ref<'_, State>, Error> {
        self.live()?;
        Ok(self.state.borrow())
    }

    #[cfg(feature = "gtk")]
    fn startup_notify_id(&self, app: Option<&gio::AppInfo>) -> Option<String> {
        self.gdk.borrow().as_ref().and_then(|gdk| {
            gdk.display
                .app_launch_context()
                .startup_notify_id(app, &[])
                .map(String::from)
        })
    }

    #[cfg(not(feature = "gtk"))]
    fn startup_notify_id(&self, _: Option<&gio::AppInfo>) -> Option<String> {
        None
    }

    #[cfg(feature = "gtk")]
    fn display_name(&self) -> Option<String> {
        self.gdk
            .borrow()
            .as_ref()
            .map(|gdk| gdk.display.name().to_string())
    }

    #[cfg(not(feature = "gtk"))]
    fn display_name(&self) -> Option<String> {
        None
    }

    /// GDK closes the display: libwayland's connection goes with it, so the source stops
    /// polling its descriptor and nothing touches the connection again.
    #[cfg(feature = "gtk")]
    fn close(&self) {
        self.closed.set(true);
        self.source.destroy();
        // wayland-backend destroys its proxies and its queue through libwayland when the
        // last reference to the connection drops, which now would be freed memory. That
        // reference is kept for the rest of the process.
        std::mem::forget(self.connection.clone());
        tracing::warn!("the display closed: the compositor client stops");
    }

    fn flush(&self) -> Result<(), Error> {
        self.live()?;
        tolerate_would_block(self.connection.flush())
            .map_err(|err| Error::Connection(err.to_string()))
    }

    /// Dispatches what other readers of the socket moved into our queue. True when there
    /// was something to deliver, or a failure for the next `pump` to report.
    fn collect(&self) -> bool {
        let (Ok(mut queue), Ok(mut state)) =
            (self.queue.try_borrow_mut(), self.state.try_borrow_mut())
        else {
            return false;
        };
        match queue.dispatch_pending(&mut state) {
            Ok(dispatched) => dispatched > 0,
            Err(err) => {
                self.failure
                    .replace(Some(Error::Connection(err.to_string())));
                true
            }
        }
    }

    fn pump(self: &Rc<Self>) -> Result<(), Error> {
        self.live()?;
        if let Some(err) = self.failure.take() {
            return Err(err);
        }
        {
            let mut queue = self.queue.borrow_mut();
            let mut state = self.state.borrow_mut();
            let failed = |err: &dyn std::fmt::Display| Error::Connection(err.to_string());
            queue
                .dispatch_pending(&mut state)
                .map_err(|err| failed(&err))?;
            if let Some(guard) = queue.prepare_read() {
                tolerate_would_block(guard.read().map(drop)).map_err(|err| failed(&err))?;
            }
            queue
                .dispatch_pending(&mut state)
                .map_err(|err| failed(&err))?;
        }
        self.flush()?;
        self.deliver();
        self.deliver_outputs();
        Ok(())
    }

    /// Hands the outputs to their handler when they differ from what it last received, with
    /// no borrow held. A nested call finds the handler taken and leaves the change to the
    /// outer one.
    fn deliver_outputs(&self) {
        let Some(handler) = self.outputs_handler.take() else {
            return;
        };
        let outputs = self.state.borrow().outputs();
        if *self.last_outputs.borrow() != outputs {
            self.last_outputs.replace(outputs.clone());
            handler(outputs);
        }
        let mut slot = self.outputs_handler.borrow_mut();
        if slot.is_none() {
            *slot = Some(handler);
        }
    }

    /// Hands the collected events to the handler with no borrow held, so the handler may
    /// call back into the client. Events a nested call collects are delivered by the
    /// outermost call, in order.
    fn deliver(self: &Rc<Self>) {
        if self.delivering.get() {
            return;
        }
        let Some(mut handler) = self.handler.take() else {
            self.state.borrow_mut().events.clear();
            return;
        };
        self.delivering.set(true);
        let client = Client {
            inner: Rc::clone(self),
        };
        loop {
            let events = std::mem::take(&mut self.state.borrow_mut().events);
            if events.is_empty() {
                break;
            }
            for event in &events {
                handler(&client, event);
            }
        }
        self.delivering.set(false);
        // A handler that connected a new handler keeps the new one.
        let mut slot = self.handler.borrow_mut();
        if slot.is_none() {
            *slot = Some(handler);
        }
    }
}

/// The GLib source of the queue: glib 0.22 offers no safe way to write one, so this is the
/// only module of the crate that speaks to glib's C interface.
mod queue_source {
    #![allow(unsafe_code)]

    use std::os::raw::c_int;
    use std::ptr;
    use std::rc::{Rc, Weak};

    use glib::ffi::{
        g_source_add_unix_fd, g_source_new, g_source_query_unix_fd, gboolean, gpointer, GSource,
        GSourceFunc, GSourceFuncs, G_IO_ERR, G_IO_HUP, G_IO_IN, G_SOURCE_CONTINUE, G_SOURCE_REMOVE,
    };
    use glib::translate::{from_glib_full, IntoGlib};

    use super::Inner;

    /// The block `g_source_new` allocates for a size of `size_of::<Block>()`: glib's header,
    /// then our fields, zeroed. No Rust reference ever covers the header, which glib mutates.
    #[repr(C)]
    struct Block {
        header: GSource,
        fields: Fields,
    }

    /// Valid when zero.
    struct Fields {
        inner: Option<Box<Weak<Inner>>>,
        /// The tag `g_source_add_unix_fd` returned for the display's descriptor.
        fd: gpointer,
    }

    impl Fields {
        fn client(&self) -> Option<Rc<Inner>> {
            self.inner.as_deref().and_then(Weak::upgrade)
        }
    }

    /// Our fields in a source's block, reached without a reference to the header.
    fn fields_of(source: *mut GSource) -> *mut Fields {
        source
            .wrapping_byte_add(std::mem::offset_of!(Block, fields))
            .cast()
    }

    static FUNCS: GSourceFuncs = GSourceFuncs {
        prepare: Some(prepare),
        check: Some(check),
        dispatch: Some(dispatch),
        finalize: Some(finalize),
        closure_callback: None,
        closure_marshal: None,
    };

    /// Creates the source and attaches it to `context`. It lives until `Inner` drops or the
    /// display closes, either of which destroys it, or until a failure makes it remove itself.
    pub(super) fn attach(
        inner: Weak<Inner>,
        fd: c_int,
        context: &glib::MainContext,
    ) -> glib::Source {
        let size = std::mem::size_of::<Block>() as u32;
        // SAFETY: glib never writes through the table, and a static outlives every source.
        let raw = unsafe { g_source_new(ptr::addr_of!(FUNCS).cast_mut(), size) };
        // SAFETY: `raw` is a live source; the descriptor stays open while the connection
        // lives, and the source is destroyed before the connection closes.
        let tag = unsafe { g_source_add_unix_fd(raw, fd, G_IO_IN | G_IO_ERR | G_IO_HUP) };
        // SAFETY: `raw` points to a block of `size` bytes whose fields are zeroed, a valid
        // `Fields`; nothing else reads them while this reference lives.
        let fields = unsafe { &mut *fields_of(raw) };
        fields.inner = Some(Box::new(inner));
        fields.fd = tag;
        // SAFETY: `raw` carries the one reference `g_source_new` returned, handed over here.
        let source: glib::Source = unsafe { from_glib_full(raw) };
        source.attach(Some(context));
        source
    }

    /// # Safety
    /// `source` is a live source created by `attach`, as glib passes to our functions.
    unsafe fn fields<'a>(source: *mut GSource) -> &'a Fields {
        // SAFETY: the caller's contract; glib holds a reference for the duration of the call,
        // and only `finalize` writes the fields again.
        unsafe { &*fields_of(source) }
    }

    /// Before the poll: ready at once when other readers left events in the queue; otherwise
    /// sends what is buffered and waits on the descriptor.
    unsafe extern "C" fn prepare(source: *mut GSource, timeout: *mut c_int) -> gboolean {
        // SAFETY: glib calls this with a source of ours.
        let fields = unsafe { fields(source) };
        let ready = fields
            .client()
            .is_some_and(|inner| inner.collect() || inner.flush().is_err());
        // SAFETY: glib passes a valid pointer to this source's poll timeout.
        unsafe { *timeout = if ready { 0 } else { -1 } };
        ready.into_glib()
    }

    /// After the poll: ready when the descriptor is readable or reports an error, or when a
    /// reader in another source's `check` filled the queue.
    unsafe extern "C" fn check(source: *mut GSource) -> gboolean {
        // SAFETY: glib calls this with a source of ours.
        let fields = unsafe { fields(source) };
        // SAFETY: `fields.fd` is the tag `g_source_add_unix_fd` returned for this source.
        let revents = unsafe { g_source_query_unix_fd(source, fields.fd) };
        let ready = revents != 0 || fields.client().is_some_and(|inner| inner.collect());
        ready.into_glib()
    }

    unsafe extern "C" fn dispatch(source: *mut GSource, _: GSourceFunc, _: gpointer) -> gboolean {
        // SAFETY: glib calls this with a source of ours.
        let Some(inner) = unsafe { fields(source) }.client() else {
            return G_SOURCE_REMOVE;
        };
        match inner.pump() {
            Ok(()) => G_SOURCE_CONTINUE,
            Err(err) => {
                tracing::error!("reading from the compositor failed, no further events: {err}");
                inner.closed.set(true);
                G_SOURCE_REMOVE
            }
        }
    }

    unsafe extern "C" fn finalize(source: *mut GSource) {
        // SAFETY: glib calls this once, when the last reference is gone, and never again
        // reads the block afterwards.
        let fields = unsafe { &mut *fields_of(source) };
        fields.inner.take();
    }
}

fn active_state(enabled: bool) -> ActiveState {
    if enabled {
        ActiveState::Enabled
    } else {
        ActiveState::Disabled
    }
}

#[derive(Default)]
pub(crate) struct Globals {
    toplevel_info: Option<ZcosmicToplevelInfoV1>,
    toplevel_manager: Option<ZcosmicToplevelManagerV1>,
    athanor_toplevel: Option<AthanorToplevelManagerV1>,
    workspace_manager: Option<ExtWorkspaceManagerV1>,
    cosmic_workspaces: Option<ZcosmicWorkspaceManagerV2>,
    seat: Option<wl_seat::WlSeat>,
    keyboard_layouts: Option<ZcosmicKeyboardLayoutManagerV1>,
    a11y: Option<CosmicA11yManagerV1>,
    security_context: Option<WpSecurityContextManagerV1>,
    /// The window pictures, GDK textures: bound only with the `gtk` feature, the capture
    /// globals at the first capture.
    #[cfg(feature = "gtk")]
    pub(crate) shm: Option<wl_shm::WlShm>,
    #[cfg(feature = "gtk")]
    pub(crate) capture: crate::capture::Globals,
}

pub(crate) struct Toplevel {
    pub(crate) ext: ExtForeignToplevelHandleV1,
    cosmic: Option<ZcosmicToplevelHandleV1>,
    athanor: Option<AthanorToplevelStateV1>,
}

struct WorkspaceHandles {
    ext: ExtWorkspaceHandleV1,
    cosmic: Option<ZcosmicWorkspaceHandleV2>,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) globals: Globals,
    windows: Table<WindowId, Window>,
    pub(crate) toplevels: HashMap<WindowId, Toplevel>,
    workspaces: Table<WorkspaceId, Workspace>,
    workspace_handles: HashMap<WorkspaceId, WorkspaceHandles>,
    /// Each group's outputs, in the order they entered, GDK's objects included.
    groups: HashMap<ObjectId, Vec<ObjectId>>,
    group_of: HashMap<WorkspaceId, ObjectId>,
    /// Output globals by registry name, their connector names, and their modes.
    outputs: HashMap<u32, wl_output::WlOutput>,
    output_names: HashMap<ObjectId, String>,
    output_modes: HashMap<ObjectId, OutputMode>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    keyboard_layout: Option<ZcosmicKeyboardLayoutV1>,
    keyboard_layouts: Vec<String>,
    keyboard_group: u32,
    accessibility: Accessibility,
    events: Vec<Event>,
    /// The window pictures in flight, by call.
    #[cfg(feature = "gtk")]
    pub(crate) captures: HashMap<u64, crate::capture::Pending>,
}

pub(crate) fn bind<I>(
    globals: &GlobalList,
    qh: &QueueHandle<State>,
    versions: RangeInclusive<u32>,
) -> Option<I>
where
    I: Proxy + 'static,
    State: Dispatch<I, ()>,
{
    match globals.bind(qh, versions, ()) {
        Ok(proxy) => Some(proxy),
        Err(err) => {
            tracing::info!(interface = I::interface().name, "not bound: {err}");
            None
        }
    }
}

impl State {
    fn bind(globals: &GlobalList, qh: &QueueHandle<State>) -> State {
        let mut state = State::default();
        // Outputs first: the workspace manager announces a group's outputs only through
        // the `wl_output` objects the client holds when it binds the manager.
        for global in globals.contents().clone_list() {
            if global.interface == wl_output::WlOutput::interface().name {
                state.bind_output(globals.registry(), qh, global.name, global.version);
            }
        }
        state.globals = Globals {
            toplevel_info: bind(globals, qh, 2..=3),
            toplevel_manager: bind(globals, qh, 1..=4),
            athanor_toplevel: bind(globals, qh, 1..=1),
            cosmic_workspaces: bind(globals, qh, 2..=2),
            keyboard_layouts: bind(globals, qh, 1..=1),
            // Version 3 deprecates the screen filter events of version 2.
            a11y: bind(globals, qh, 2..=2),
            security_context: bind(globals, qh, 1..=1),
            #[cfg(feature = "gtk")]
            shm: bind(globals, qh, 1..=1),
            #[cfg(feature = "gtk")]
            capture: crate::capture::Globals::default(),
            seat: bind(globals, qh, 1..=7),
            workspace_manager: bind(globals, qh, 1..=1),
        };
        // Only its events are needed; the proxy stays alive without a handle.
        bind::<ExtForeignToplevelListV1>(globals, qh, 1..=1);
        state
    }

    fn bind_output(
        &mut self,
        registry: &wl_registry::WlRegistry,
        qh: &QueueHandle<State>,
        name: u32,
        version: u32,
    ) {
        let output = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
        self.outputs.insert(name, output);
    }

    fn outputs(&self) -> Vec<Output> {
        let mut names: Vec<_> = self.outputs.keys().copied().collect();
        names.sort_unstable();
        names
            .iter()
            .filter_map(|name| self.outputs.get(name))
            .map(|output| {
                let (width, height) = self
                    .output_modes
                    .get(&output.id())
                    .copied()
                    .unwrap_or_default()
                    .logical();
                Output {
                    connector: self.output_names.get(&output.id()).cloned(),
                    width,
                    height,
                }
            })
            .collect()
    }

    fn seat(&self) -> Result<&wl_seat::WlSeat, Error> {
        self.globals
            .seat
            .as_ref()
            .ok_or(Error::Unavailable("wl_seat"))
    }

    fn cosmic_toplevel(&self, window: WindowId) -> Result<&ZcosmicToplevelHandleV1, Error> {
        self.toplevels
            .get(&window)
            .ok_or(Error::NoWindow)?
            .cosmic
            .as_ref()
            .ok_or(Error::Unavailable("zcosmic_toplevel_info_v1"))
    }

    fn a11y(&self) -> Result<&CosmicA11yManagerV1, Error> {
        self.globals
            .a11y
            .as_ref()
            .ok_or(Error::Unavailable("cosmic_a11y_manager_v1"))
    }

    // The compositor names an output in reply to its bind, before it can report a toplevel
    // on it.
    fn output_enter(&mut self, id: &WindowId, output: &wl_output::WlOutput) {
        let name = self.output_names.get(&output.id());
        if let (Some(window), Some(name)) = (self.windows.pending(id), name) {
            if !window.outputs.contains(name) {
                window.outputs.push(name.clone());
            }
        }
    }

    fn output_leave(&mut self, id: &WindowId, output: &wl_output::WlOutput) {
        let name = self.output_names.get(&output.id());
        if let (Some(window), Some(name)) = (self.windows.pending(id), name) {
            window.outputs.retain(|entered| entered != name);
        }
    }

    fn push_window(&mut self, change: Change<Window>) {
        self.events.push(match change {
            Change::Added(window) => Event::WindowAdded(window),
            Change::Changed(window) => Event::WindowChanged(window),
        });
    }

    fn commit_workspaces(&mut self) {
        for workspace in self.workspaces.pending_values() {
            workspace.output = self
                .group_of
                .get(&workspace.id)
                .and_then(|group| self.groups.get(group))
                // The group enters every `wl_output` of this connection, GDK's too; only
                // ours carry a name this client has read.
                .and_then(|outputs| {
                    outputs
                        .iter()
                        .find_map(|output| self.output_names.get(output))
                })
                .cloned();
        }
        for change in self.workspaces.commit_all() {
            self.events.push(match change {
                Change::Added(workspace) => Event::WorkspaceAdded(workspace),
                Change::Changed(workspace) => Event::WorkspaceChanged(workspace),
            });
        }
    }

    fn set_accessibility(&mut self, next: Accessibility) {
        if next != self.accessibility {
            self.accessibility = next;
            self.events.push(Event::Accessibility(next));
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == wl_output::WlOutput::interface().name => {
                state.bind_output(registry, qh, name, version);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(output) = state.outputs.remove(&name) {
                    state.output_names.remove(&output.id());
                    state.output_modes.remove(&output.id());
                    if output.version() >= 3 {
                        output.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mode = state.output_modes.entry(output.id()).or_default();
        match event {
            wl_output::Event::Name { name } => {
                state.output_names.insert(output.id(), name);
            }
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => mode.size = (width, height),
            wl_output::Event::Scale { factor } => mode.scale = factor,
            wl_output::Event::Geometry {
                transform: WEnum::Value(transform),
                ..
            } => mode.transform = Some(transform),
            _ => {}
        }
    }
}

/// What `wl_output` says of an output's size: its current mode, scale and transform.
#[derive(Clone, Copy, Debug, Default)]
struct OutputMode {
    size: (i32, i32),
    scale: i32,
    transform: Option<wl_output::Transform>,
}

impl OutputMode {
    /// The logical size: the mode turned by the transform, divided by the scale.
    fn logical(self) -> (i32, i32) {
        use wl_output::Transform;
        let (width, height) = self.size;
        let (width, height) = match self.transform {
            Some(Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270) => {
                (height, width)
            }
            _ => (width, height),
        };
        let scale = self.scale.max(1);
        (width / scale, height / scale)
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };
        if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
            let keyboard = seat.get_keyboard(qh, ());
            if let Some(manager) = &state.globals.keyboard_layouts {
                state.keyboard_layout = Some(manager.get_keyboard_layout(&keyboard, qh, ()));
            }
            state.keyboard = Some(keyboard);
        }
    }
}

fn read_keymap(fd: OwnedFd, size: u32) -> std::io::Result<String> {
    if size > MAX_KEYMAP {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            format!("a keymap of {size} bytes is beyond the limit of {MAX_KEYMAP}"),
        ));
    }
    let mut bytes = vec![0; size as usize];
    File::from(fd).read_exact_at(&mut bytes, 0)?;
    let text = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    Ok(String::from_utf8_lossy(text).into_owned())
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wl_keyboard::Event::Keymap { format, fd, size } = event else {
            return;
        };
        if format != WEnum::Value(wl_keyboard::KeymapFormat::XkbV1) {
            return;
        }
        match read_keymap(fd, size) {
            Ok(text) => {
                let layouts = keymap::layout_names(&text);
                if layouts != state.keyboard_layouts {
                    state.keyboard_layouts = layouts.clone();
                    state.events.push(Event::KeyboardLayouts(layouts));
                }
            }
            Err(err) => tracing::warn!("the keymap could not be read: {err}"),
        }
    }
}

impl Dispatch<ZcosmicKeyboardLayoutV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicKeyboardLayoutV1,
        event: zcosmic_keyboard_layout_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zcosmic_keyboard_layout_v1::Event::Group { group } = event;
        if group != state.keyboard_group {
            state.keyboard_group = group;
            state.events.push(Event::KeyboardGroup(group));
        }
    }
}

impl Dispatch<CosmicA11yManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &CosmicA11yManagerV1,
        event: cosmic_a11y_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let enabled = |value: WEnum<ActiveState>| value == WEnum::Value(ActiveState::Enabled);
        let mut next = state.accessibility;
        match event {
            cosmic_a11y_manager_v1::Event::Magnifier { active } => next.magnifier = enabled(active),
            cosmic_a11y_manager_v1::Event::ScreenFilter { inverted, filter } => {
                next.inverted = enabled(inverted);
                next.filter = match filter {
                    WEnum::Value(Filter::Disabled) => ScreenFilter::None,
                    WEnum::Value(Filter::Greyscale) => ScreenFilter::Greyscale,
                    WEnum::Value(Filter::DaltonizeProtanopia) => ScreenFilter::Protanopia,
                    WEnum::Value(Filter::DaltonizeDeuteranopia) => ScreenFilter::Deuteranopia,
                    WEnum::Value(Filter::DaltonizeTritanopia) => ScreenFilter::Tritanopia,
                    WEnum::Value(Filter::Unknown) | WEnum::Unknown(_) => ScreenFilter::Unknown,
                };
            }
            // Version 3 only; this client binds version 2.
            cosmic_a11y_manager_v1::Event::ScreenFilter2 { .. } => {}
        }
        state.set_accessibility(next);
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        let Some(&id) = toplevel.data::<WindowId>() else {
            return;
        };
        let cosmic = state
            .globals
            .toplevel_info
            .as_ref()
            .map(|info| info.get_cosmic_toplevel(&toplevel, qh, id));
        // COSMIC's extension first (plan D3). One state object per window: the compositor
        // ends the connection on a second.
        let athanor = match (&cosmic, &state.globals.athanor_toplevel) {
            (None, Some(manager)) => Some(manager.get_toplevel_state(&toplevel, qh, id)),
            _ => None,
        };
        state.windows.insert(
            id,
            Window {
                id,
                app_id: String::new(),
                title: String::new(),
                state: WindowState::default(),
                outputs: Vec::new(),
            },
        );
        state.toplevels.insert(
            id,
            Toplevel {
                ext: toplevel,
                cosmic,
                athanor,
            },
        );
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, WindowId(fresh()))
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(window) = state.windows.pending(id) {
                    window.title = title;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(window) = state.windows.pending(id) {
                    window.app_id = app_id;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(change) = state.windows.commit(id) {
                    state.push_window(change);
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                // A capture ends before the handle it was made from does.
                #[cfg(feature = "gtk")]
                state.captures.retain(|_, capture| {
                    if capture.window != *id {
                        return true;
                    }
                    capture.wake();
                    false
                });
                if let Some(toplevel) = state.toplevels.remove(id) {
                    if let Some(cosmic) = toplevel.cosmic {
                        cosmic.destroy();
                    }
                    if let Some(athanor) = toplevel.athanor {
                        athanor.destroy();
                    }
                    toplevel.ext.destroy();
                }
                if state.windows.remove(id) {
                    state.events.push(Event::WindowRemoved(*id));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicToplevelInfoV1,
        event: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_toplevel_info_v1::Event::Done = event {
            for change in state.windows.commit_all() {
                state.push_window(change);
            }
        }
    }

    // Version 1 announced toplevels itself; a client binding version 2 never receives it.
    event_created_child!(State, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, WindowId(fresh()))
    ]);
}

impl Dispatch<ZcosmicToplevelHandleV1, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zcosmic_toplevel_handle_v1::Event::State { state: array } => {
                if let Some(window) = state.windows.pending(id) {
                    window.state = WindowState::from_cosmic(&array);
                }
            }
            zcosmic_toplevel_handle_v1::Event::OutputEnter { output } => {
                state.output_enter(id, &output);
            }
            zcosmic_toplevel_handle_v1::Event::OutputLeave { output } => {
                state.output_leave(id, &output);
            }
            _ => {}
        }
    }
}

impl Dispatch<AthanorToplevelStateV1, WindowId> for State {
    fn event(
        state: &mut Self,
        _: &AthanorToplevelStateV1,
        event: athanor_toplevel_state_v1::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            athanor_toplevel_state_v1::Event::State { state: flags } => {
                let flags = match flags {
                    WEnum::Value(flags) => flags.bits(),
                    WEnum::Unknown(flags) => flags,
                };
                if let Some(window) = state.windows.pending(id) {
                    window.state = WindowState::from_athanor(flags);
                }
            }
            athanor_toplevel_state_v1::Event::OutputEnter { output } => {
                state.output_enter(id, &output);
            }
            athanor_toplevel_state_v1::Event::OutputLeave { output } => {
                state.output_leave(id, &output);
            }
            athanor_toplevel_state_v1::Event::Done => {
                if let Some(change) = state.windows.commit(id) {
                    state.push_window(change);
                }
            }
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.groups.insert(workspace_group.id(), Vec::new());
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                let Some(&id) = workspace.data::<WorkspaceId>() else {
                    return;
                };
                let cosmic = state
                    .globals
                    .cosmic_workspaces
                    .as_ref()
                    .map(|manager| manager.get_cosmic_workspace(&workspace, qh, id));
                state.workspaces.insert(
                    id,
                    Workspace {
                        id,
                        name: String::new(),
                        active: false,
                        tiling: None,
                        output: None,
                    },
                );
                state.workspace_handles.insert(
                    id,
                    WorkspaceHandles {
                        ext: workspace,
                        cosmic,
                    },
                );
            }
            ext_workspace_manager_v1::Event::Done => state.commit_workspaces(),
            _ => {}
        }
    }

    event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, WorkspaceId(fresh()))
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => {
                if let Some(outputs) = state.groups.get_mut(&group.id()) {
                    outputs.push(output.id());
                }
            }
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => {
                if let Some(outputs) = state.groups.get_mut(&group.id()) {
                    outputs.retain(|entered| *entered != output.id());
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                if let Some(&id) = workspace.data::<WorkspaceId>() {
                    state.group_of.insert(id, group.id());
                }
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                if let Some(id) = workspace.data::<WorkspaceId>() {
                    if state.group_of.get(id) == Some(&group.id()) {
                        state.group_of.remove(id);
                    }
                }
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                state.groups.remove(&group.id());
                group.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_handle_v1::Event::Name { name } => {
                if let Some(workspace) = state.workspaces.pending(id) {
                    workspace.name = name;
                }
            }
            ext_workspace_handle_v1::Event::State { state: bits } => {
                if let Some(workspace) = state.workspaces.pending(id) {
                    workspace.active =
                        u32::from(bits) & ext_workspace_handle_v1::State::Active.bits() != 0;
                }
            }
            ext_workspace_handle_v1::Event::Removed => {
                if let Some(handles) = state.workspace_handles.remove(id) {
                    if let Some(cosmic) = handles.cosmic {
                        cosmic.destroy();
                    }
                    handles.ext.destroy();
                }
                state.group_of.remove(id);
                if state.workspaces.remove(id) {
                    state.events.push(Event::WorkspaceRemoved(*id));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicWorkspaceHandleV2, WorkspaceId> for State {
    fn event(
        state: &mut Self,
        _: &ZcosmicWorkspaceHandleV2,
        event: zcosmic_workspace_handle_v2::Event,
        id: &WorkspaceId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zcosmic_workspace_handle_v2::Event::TilingState { state: tiling } = event {
            if let Some(workspace) = state.workspaces.pending(id) {
                workspace.tiling = match tiling {
                    WEnum::Value(TilingState::FloatingOnly) => Some(Tiling::Floating),
                    WEnum::Value(TilingState::TilingEnabled) => Some(Tiling::Tiled),
                    WEnum::Unknown(_) => None,
                };
            }
        }
    }
}

/// Globals and objects whose events this client does not read.
macro_rules! ignore_events {
    ($($proxy:ty),* $(,)?) => {$(
        impl Dispatch<$proxy, ()> for State {
            fn event(
                _: &mut Self,
                _: &$proxy,
                _: <$proxy as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events!(
    ZcosmicToplevelManagerV1,
    AthanorToplevelManagerV1,
    ZcosmicWorkspaceManagerV2,
    ZcosmicKeyboardLayoutManagerV1,
    WpSecurityContextManagerV1,
    WpSecurityContextV1,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_full_socket_is_tolerated() {
        let io = |kind| Err(WaylandError::Io(std::io::Error::from(kind)));
        assert!(tolerate_would_block(Ok(())).is_ok());
        assert!(tolerate_would_block(io(ErrorKind::WouldBlock)).is_ok());
        assert!(tolerate_would_block(io(ErrorKind::BrokenPipe)).is_err());
        assert!(tolerate_would_block(io(ErrorKind::ConnectionReset)).is_err());
    }

    #[test]
    fn the_logical_size_turns_with_the_transform_and_divides_by_the_scale() {
        use wl_output::Transform;
        let mode = |transform, scale| OutputMode {
            size: (2560, 1600),
            scale,
            transform,
        };
        assert_eq!(mode(None, 0).logical(), (2560, 1600));
        assert_eq!(mode(Some(Transform::Normal), 2).logical(), (1280, 800));
        assert_eq!(mode(Some(Transform::_90), 2).logical(), (800, 1280));
        assert_eq!(mode(Some(Transform::Flipped270), 1).logical(), (1600, 2560));
        assert_eq!(mode(Some(Transform::_180), 1).logical(), (2560, 1600));
        assert_eq!(OutputMode::default().logical(), (0, 0));
    }
}
