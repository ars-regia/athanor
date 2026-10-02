//! D-Bus for the system modules (doc_bar.md, BR3): calls with a timeout, and a mirror of one
//! service's objects kept current by its signals. The mirror follows the service's unique
//! owner: a restart is a new owner, a new generation and a fresh load, and a signal from any
//! other sender is ignored. Every call allows interactive authorisation, so polkit can ask.

use std::cell::{Cell, Ref, RefCell};
use std::future::Future;
use std::rc::Rc;

use athanor_bar::props::{self, Objects};
use gtk4::prelude::*;
use gtk4::{gio, glib};

pub const TIMEOUT_MS: i32 = 5000;
/// A call that waits for the person: a pairing, or a connection polkit asks about.
pub const INTERACTIVE_TIMEOUT_MS: i32 = 120_000;
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

pub fn call(
    connection: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    method: &str,
    args: Option<&glib::Variant>,
    timeout: i32,
) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static {
    connection.call_future(
        Some(name),
        path,
        interface,
        method,
        args,
        None,
        gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
        timeout,
    )
}

pub fn set_property(
    connection: &gio::DBusConnection,
    name: &str,
    path: &str,
    interface: &str,
    property: &str,
    value: glib::Variant,
) -> impl Future<Output = Result<glib::Variant, glib::Error>> + 'static {
    let args = (interface, property, value).to_variant();
    call(
        connection,
        name,
        path,
        PROPERTIES,
        "Set",
        Some(&args),
        TIMEOUT_MS,
    )
}

/// Runs a call the bar makes on its own, whose reply nobody reads; a failure is logged with
/// the action's name only. The arguments are never logged: one of them may be a password.
pub fn spawn(
    action: &'static str,
    future: impl Future<Output = Result<glib::Variant, glib::Error>> + 'static,
) {
    glib::spawn_future_local(async move {
        if let Err(err) = future.await {
            refused(action, &err);
        }
    });
}

/// Runs a call the person asked for. A failure is logged as by [`spawn`], then `failed` runs:
/// the module shows its service's state again, since no change arrives that would put a
/// switch or a slider back, and says in the popover that the action did not complete.
pub fn act(
    action: &'static str,
    future: impl Future<Output = Result<glib::Variant, glib::Error>> + 'static,
    failed: impl FnOnce() + 'static,
) {
    glib::spawn_future_local(async move {
        if let Err(err) = future.await {
            refused(action, &err);
            failed();
        }
    });
}

fn refused(action: &str, err: &glib::Error) {
    tracing::warn!(error = %err, action, "a system service refused or did not answer");
}

/// What the mirror loads.
pub enum Source {
    /// `GetManagedObjects` at this path, then `InterfacesAdded`, `InterfacesRemoved` and
    /// `PropertiesChanged`.
    Managed(&'static str),
    /// `GetAll` of each (path, interface), then `PropertiesChanged`.
    Fixed(Vec<(&'static str, &'static str)>),
}

pub struct Mirror {
    connection: gio::DBusConnection,
    name: String,
    source: Source,
    objects: RefCell<Objects>,
    owner: RefCell<Option<String>>,
    generation: Cell<u64>,
    scheduled: Cell<bool>,
    notify: Box<dyn Fn()>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    /// Ends the name watch. A closure, because gio 0.22 cannot name the watcher id's type
    /// (`dbus::WatcherId` is shadowed by `dbus_connection::WatcherId`).
    unwatch: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Mirror {
    /// Mirrors `name` on `connection`. `notify` runs on an idle callback after a change,
    /// coalesced, never inside a signal handler.
    pub fn new(
        connection: &gio::DBusConnection,
        name: &str,
        source: Source,
        notify: impl Fn() + 'static,
    ) -> Rc<Mirror> {
        let mirror = Rc::new(Mirror {
            connection: connection.clone(),
            name: name.to_owned(),
            source,
            objects: RefCell::new(Objects::new()),
            owner: RefCell::new(None),
            generation: Cell::new(0),
            scheduled: Cell::new(false),
            notify: Box::new(notify),
            subscription: RefCell::new(None),
            unwatch: RefCell::new(None),
        });
        let weak = Rc::downgrade(&mirror);
        let subscription = connection.subscribe_to_signal(
            Some(name),
            None,
            None,
            None,
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(mirror) = weak.upgrade() {
                    mirror.signal(&signal);
                }
            },
        );
        mirror.subscription.replace(Some(subscription));
        let (appeared, vanished) = (Rc::downgrade(&mirror), Rc::downgrade(&mirror));
        let watcher = gio::bus_watch_name_on_connection(
            connection,
            name,
            gio::BusNameWatcherFlags::NONE,
            move |_, _, owner| {
                if let Some(mirror) = appeared.upgrade() {
                    mirror.appeared(owner);
                }
            },
            move |_, _| {
                if let Some(mirror) = vanished.upgrade() {
                    mirror.vanished();
                }
            },
        );
        mirror
            .unwatch
            .replace(Some(Box::new(move || gio::bus_unwatch_name(watcher))));
        mirror
    }

    pub fn objects(&self) -> Ref<'_, Objects> {
        self.objects.borrow()
    }

    pub fn owner(&self) -> Option<String> {
        self.owner.borrow().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Whether `sender` is the service's current unique name.
    pub fn is_owner(&self, sender: &str) -> bool {
        self.owner.borrow().as_deref() == Some(sender)
    }

    pub fn connection(&self) -> &gio::DBusConnection {
        &self.connection
    }

    fn signal(self: &Rc<Self>, signal: &gio::DBusSignalRef<'_>) {
        if !self.is_owner(signal.sender_name) {
            return;
        }
        let Some(change) = props::change(
            signal.object_path,
            signal.interface_name,
            signal.signal_name,
            signal.parameters,
        ) else {
            return;
        };
        props::apply(&mut self.objects.borrow_mut(), change);
        self.schedule();
    }

    fn appeared(self: &Rc<Self>, owner: &str) {
        self.owner.replace(Some(owner.to_owned()));
        self.generation.set(self.generation.get() + 1);
        self.objects.borrow_mut().clear();
        self.schedule();
        self.load(owner);
    }

    fn vanished(self: &Rc<Self>) {
        if self.owner.replace(None).is_some() {
            tracing::info!(name = %self.name, "the service left the bus");
        }
        self.generation.set(self.generation.get() + 1);
        self.objects.borrow_mut().clear();
        self.schedule();
    }

    /// Loads the objects from `owner`, the unique name: a reply always comes from the owner
    /// this generation belongs to.
    fn load(self: &Rc<Self>, owner: &str) {
        let generation = self.generation.get();
        let requests: Vec<(&'static str, Option<&'static str>)> = match &self.source {
            Source::Managed(root) => vec![(*root, None)],
            Source::Fixed(list) => list
                .iter()
                .map(|(path, interface)| (*path, Some(*interface)))
                .collect(),
        };
        for (path, interface) in requests {
            let reply = match interface {
                None => call(
                    &self.connection,
                    owner,
                    path,
                    OBJECT_MANAGER,
                    "GetManagedObjects",
                    None,
                    TIMEOUT_MS,
                ),
                Some(interface) => {
                    let args = (interface,).to_variant();
                    call(
                        &self.connection,
                        owner,
                        path,
                        PROPERTIES,
                        "GetAll",
                        Some(&args),
                        TIMEOUT_MS,
                    )
                }
            };
            let weak = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let reply = reply.await;
                let Some(mirror) = weak.upgrade() else { return };
                if mirror.generation.get() != generation {
                    return;
                }
                let reply = match reply {
                    Ok(reply) => reply,
                    Err(err) => {
                        tracing::warn!(error = %err, name = %mirror.name, path, "the service did not answer; it is shown as absent");
                        return;
                    }
                };
                match interface {
                    None => match props::managed_objects(&reply) {
                        Some(objects) => {
                            mirror.objects.replace(objects);
                        }
                        None => {
                            tracing::error!(name = %mirror.name, "GetManagedObjects answered with an unexpected type")
                        }
                    },
                    Some(interface) => match props::get_all(&reply) {
                        Some(values) => {
                            mirror
                                .objects
                                .borrow_mut()
                                .entry(path.to_owned())
                                .or_default()
                                .insert(interface.to_owned(), values);
                        }
                        None => {
                            tracing::error!(name = %mirror.name, path, "GetAll answered with an unexpected type")
                        }
                    },
                }
                mirror.schedule();
            });
        }
    }

    fn schedule(self: &Rc<Self>) {
        if self.scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(mirror) = weak.upgrade() {
                mirror.scheduled.set(false);
                (mirror.notify)();
            }
        });
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        // The subscription unsubscribes itself when dropped with the struct.
        if let Some(unwatch) = self.unwatch.take() {
            unwatch();
        }
    }
}
