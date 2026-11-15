//! athanor-bar: the bar of doc_bar.md. athanor-bar.service runs it. `--record-exit` is the
//! unit's ExecStopPost: it counts a failed run towards the crash-loop limit (doc_shell.md SH8).

mod first_layout;
mod layer_guard;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use athanor_bar::i18n;
use athanor_bar::start::{self, Prepared};
use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "os.athanor.Bar";

fn main() -> glib::ExitCode {
    let (source, favorites_file, first_session) = match start::prepare() {
        Prepared::Run {
            source,
            favorites_file,
            first_session,
        } => (source, favorites_file, first_session),
        Prepared::Exit(true) => return glib::ExitCode::SUCCESS,
        Prepared::Exit(false) => return glib::ExitCode::FAILURE,
    };

    let app = Application::builder().application_id(APP_ID).build();
    // A second `athanor-bar` activates this one and exits; the bar is built once. The
    // handle is kept for the app's lifetime: every live-reload watch holds only a weak
    // reference to the bar, so nothing else keeps it alive once `connect_activate`
    // returns.
    let handle: Rc<RefCell<Option<Rc<ui::Bar>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if handle.borrow().is_none() {
            if let (Some((paths, state)), Some(display)) =
                (first_session.clone(), gtk4::gdk::Display::default())
            {
                first_layout::arm(&display, paths, start::first_session_marker(&state));
            }
            let bar = ui::start(app, source.clone(), favorites_file.clone());
            handle.replace(Some(bar));
        }
    });
    app.run_with_args(&Vec::<String>::new())
}
