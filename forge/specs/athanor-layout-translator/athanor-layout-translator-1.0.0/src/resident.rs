//! The translator for the life of the session (doc_shell.md, SH7): it re-renders when
//! the document or an output changes, and a pass that changes nothing writes nothing.
//!
//! Events are coalesced: a burst (a rotation reports a geometry change per monitor; an
//! editor's save is a create and a rename) runs one pass, DEBOUNCE after the last event.

use std::cell::RefCell;
use std::error::Error;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

use crate::{notify_ready, outputs, pass, Dirs};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct Resident {
    dirs: Dirs,
    display: gdk::Display,
    pending: RefCell<Option<glib::SourceId>>,
    /// Kept alive for as long as the translator runs: a dropped monitor stops watching.
    file_monitors: RefCell<Vec<gio::FileMonitor>>,
}

impl Resident {
    /// Arms the watchers, runs the first pass, and tells systemd. A first pass that
    /// cannot write is an error: systemd restarts the unit and the failure counts.
    pub fn start(dirs: Dirs, display: gdk::Display) -> Result<Rc<Resident>, Box<dyn Error>> {
        if let Some(dir) = dirs.paths.user_file.parent() {
            fs::create_dir_all(dir)?;
        }
        let resident = Rc::new(Resident {
            dirs,
            display,
            pending: RefCell::new(None),
            file_monitors: RefCell::new(Vec::new()),
        });
        resident.watch_files()?;
        resident.watch_outputs();
        pass(&resident.dirs, &outputs(&resident.display))?;
        notify_ready();
        Ok(resident)
    }

    /// The vendor and policy directories and the directory of the user document. A
    /// directory that does not exist yet -- /etc/athanor/layout on most machines -- is
    /// watched too: GIO reports it when it appears.
    fn watch_files(self: &Rc<Self>) -> Result<(), glib::Error> {
        let user_dir = self
            .dirs
            .paths
            .user_file
            .parent()
            .map(|dir| dir.to_path_buf());
        let dirs = [
            Some(self.dirs.paths.vendor_dir.clone()),
            Some(self.dirs.paths.policy_dir.clone()),
            user_dir,
        ];
        for dir in dirs.into_iter().flatten() {
            let monitor = gio::File::for_path(&dir)
                .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)?;
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                if let Some(resident) = weak.upgrade() {
                    resident.schedule();
                }
            });
            self.file_monitors.borrow_mut().push(monitor);
        }
        Ok(())
    }

    /// Outputs added and removed, and any property of an output changing: its geometry
    /// on a rotation or a mode change, its connector once it is known.
    fn watch_outputs(self: &Rc<Self>) {
        let monitors = self.display.monitors();
        for index in 0..monitors.n_items() {
            self.watch_output(monitors.item(index));
        }
        let weak = Rc::downgrade(self);
        monitors.connect_items_changed(move |monitors, position, _removed, added| {
            let Some(resident) = weak.upgrade() else {
                return;
            };
            for index in position..position + added {
                resident.watch_output(monitors.item(index));
            }
            resident.schedule();
        });
    }

    fn watch_output(self: &Rc<Self>, item: Option<glib::Object>) {
        let Some(monitor) = item.and_downcast::<gdk::Monitor>() else {
            return;
        };
        let weak = Rc::downgrade(self);
        monitor.connect_notify_local(None, move |_, _| {
            if let Some(resident) = weak.upgrade() {
                resident.schedule();
            }
        });
    }

    fn schedule(self: &Rc<Self>) {
        if let Some(previous) = self.pending.borrow_mut().take() {
            previous.remove();
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(DEBOUNCE, move || {
            let Some(resident) = weak.upgrade() else {
                return;
            };
            // The source has fired and is gone: forget it before anything can reschedule.
            resident.pending.borrow_mut().take();
            if let Err(err) = pass(&resident.dirs, &outputs(&resident.display)) {
                tracing::error!(error = %err, "cannot write the cosmic-panel configuration");
                // systemd restarts the unit and counts the failure (SH8).
                std::process::exit(1);
            }
        });
        *self.pending.borrow_mut() = Some(source);
    }
}
