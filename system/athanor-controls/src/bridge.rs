//! The pages' side of the models in athanor-services (doc_control_center.md, CC3): the models
//! on the runtime `main` started, and the loop that applies their state on the GLib main loop.
//! A reply is decoded on the runtime's thread; the interface only awaits the channel.

use std::cell::RefCell;
use std::path::PathBuf;

use athanor_services::Buses;
use gtk4::glib;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

use crate::Services;

/// What `install` keeps until the first page asks for the models, and the models once it did.
enum Models {
    Absent,
    Installed(Handle, Buses, PathBuf),
    Started(Services),
}

thread_local! {
    static MODELS: RefCell<Models> = const { RefCell::new(Models::Absent) };
}

/// Keeps the handle of the runtime `main` started, with the one set of bus connections the
/// models share, for the first page that needs a model. The models start then, not now.
pub fn install(handle: Handle, backlight_root: PathBuf) {
    let buses = Buses::new(handle.clone());
    MODELS.with(|cell| cell.replace(Models::Installed(handle, buses, backlight_root)));
}

/// The models, started on the first call; `None` when the runtime could not start, and then
/// the modules that need a model are hidden.
pub fn services() -> Option<Services> {
    MODELS.with(|cell| {
        let mut models = cell.borrow_mut();
        if let Models::Installed(handle, buses, backlight_root) = &*models {
            let started = Services::start(handle, buses, backlight_root.clone());
            *models = Models::Started(started);
        }
        match &*models {
            Models::Started(services) => Some(services.clone()),
            _ => None,
        }
    })
}

/// Applies the model's state now and after every change, on the main context, until the
/// model ends.
pub fn follow<T: Clone + 'static>(mut rx: watch::Receiver<T>, apply: impl Fn(&T) + 'static) {
    glib::spawn_future_local(async move {
        loop {
            // Cloned so no borrow of the channel is held while `apply` runs.
            let state = rx.borrow_and_update().clone();
            apply(&state);
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
}

/// Hands each item of the model's channel to `handle` on the main context, until the model
/// ends.
pub fn drain<T: 'static>(mut rx: mpsc::Receiver<T>, handle: impl Fn(T) + 'static) {
    glib::spawn_future_local(async move {
        while let Some(item) = rx.recv().await {
            handle(item);
        }
    });
}
