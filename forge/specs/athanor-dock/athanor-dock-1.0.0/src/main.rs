//! athanor-dock: the dock of doc_bar.md, BR7. athanor-dock.service runs it. `--record-exit`
//! is the unit's ExecStopPost: it counts a failed run towards the crash-loop limit
//! (doc_shell.md SH8).

mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use athanor_dock::start::{self, Prepared};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Dock";

fn main() -> glib::ExitCode {
    let (source, favorites_file) = match start::prepare() {
        Prepared::Run {
            source,
            favorites_file,
        } => (source, favorites_file),
        Prepared::Exit(true) => return glib::ExitCode::SUCCESS,
        Prepared::Exit(false) => return glib::ExitCode::FAILURE,
    };
    let app = Application::builder().application_id(APP_ID).build();
    // A second `athanor-dock` activates this one and exits; the dock is built once. The
    // handle is kept for the app's lifetime: every watch holds only a weak reference to
    // the dock.
    let handle: Rc<RefCell<Option<Rc<ui::Dock>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if handle.borrow().is_none() {
            let dock = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(dock));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
