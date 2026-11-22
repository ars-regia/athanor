//! The rig's check of Settings' resident window (doc_settings.md SE5 and section 7, SL1;
//! `forge/test/shell/toolkit_window_check.py`): Settings' window at the landing page, run as its
//! unit runs it but without the bus name, the forwarder or Landlock, and each line of its
//! standard input queued as the target of a `Show`, as `os.athanor.Settings1` queues one. Not
//! shipped.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::BufRead;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use athanor_sal::driver;
use athanor_settings::window::{First, Settings};
use athanor_settings::{i18n, look};
use athanor_settings_core::registry::Registry;
use athanor_settings_core::target::Target;
use athanor_style::appearance::Appearance;
use athanor_sulphur_client::glib_loop::GlibSource;

fn main() -> ExitCode {
    i18n::init();
    let glib = match GlibSource::new() {
        Ok(glib) => glib,
        Err(error) => {
            eprintln!("resident_probe: {error}");
            return ExitCode::FAILURE;
        }
    };
    let lines: Arc<Mutex<Vec<String>>> = Arc::default();
    let read = Arc::clone(&lines);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            match read.lock() {
                Ok(mut lines) => lines.push(line),
                Err(_) => return,
            }
        }
    });
    // The queue is the main thread's, as the bus's is: the lines join it on GLib's main context.
    let shows: Rc<RefCell<VecDeque<String>>> = Rc::default();
    let queue = Rc::clone(&shows);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let Ok(mut lines) = lines.lock() else {
            eprintln!("resident_probe: the standard input's reader stopped");
            std::process::exit(1);
        };
        queue.borrow_mut().extend(lines.drain(..));
        glib::ControlFlow::Continue
    });
    let settings = Settings::new(
        Registry::new(),
        look::look(&Appearance::read()),
        i18n::is_rtl(),
        First::Show(Target::Landing),
        Rc::default(),
        shows,
        None,
    );
    match driver::run_with(settings, glib) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("resident_probe: {error}");
            ExitCode::FAILURE
        }
    }
}
