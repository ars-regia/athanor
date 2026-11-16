//! The outputs and their shape, as GDK reports them (doc_bar.md, BR7). GDK already binds
//! `wl_output` and `xdg_output`; reading its monitors keeps one view of the outputs in
//! the process.

use std::cell::RefCell;
use std::rc::Rc;

use athanor_layout::placement::Output;
use gtk4::{gdk, prelude::*};

/// The outputs now, in logical pixels.
pub fn current(display: &gdk::Display) -> Vec<Output> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
        .map(|monitor| {
            let geometry = monitor.geometry();
            Output {
                connector: monitor.connector().map(|connector| connector.to_string()),
                width: geometry.width(),
                height: geometry.height(),
            }
        })
        .collect()
}

/// Calls `on_change` with every output each time one is added or removed, or changes
/// size, rotation, scale or connector. The watch lasts as long as the display.
pub fn watch(display: &gdk::Display, on_change: impl Fn(Vec<Output>) + 'static) {
    let weak = display.downgrade();
    let last = RefCell::new(current(display));
    let changed: Rc<dyn Fn()> = Rc::new(move || {
        let Some(display) = weak.upgrade() else {
            return;
        };
        let outputs = current(&display);
        if *last.borrow() != outputs {
            last.replace(outputs.clone());
            on_change(outputs);
        }
    });
    let follow = {
        let changed = changed.clone();
        move |monitor: gdk::Monitor| {
            let changed = changed.clone();
            monitor.connect_notify_local(None, move |_, _| changed());
        }
    };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
        .for_each(&follow);
    monitors.connect_items_changed(move |monitors, position, _, added| {
        (position..position.saturating_add(added))
            .filter_map(|index| monitors.item(index).and_downcast::<gdk::Monitor>())
            .for_each(&follow);
        changed();
    });
}
