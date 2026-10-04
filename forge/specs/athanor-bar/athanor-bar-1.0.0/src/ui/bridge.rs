//! The bar's side of the models in athanor-services (doc_control_center.md, CC3): the handle
//! of the runtime they run on, and the loop that applies their state on the GLib main loop.
//! A reply is decoded on the runtime's thread; the interface only awaits the channel.

use std::cell::RefCell;

use athanor_services::Buses;
use gtk4::glib;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

thread_local! {
    static RUNTIME: RefCell<Option<(Handle, Buses)>> = const { RefCell::new(None) };
}

/// Keeps the handle of the runtime `main` started, with the one set of bus connections the
/// models share, for the modules that start a model.
pub fn install(handle: Handle) {
    let buses = Buses::new(handle.clone());
    RUNTIME.with(|cell| cell.replace(Some((handle, buses))));
}

/// The runtime's handle and the shared buses; `None` when the runtime could not start, and
/// then the modules that need a model are hidden.
pub fn runtime() -> Option<(Handle, Buses)> {
    RUNTIME.with(|cell| cell.borrow().clone())
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
