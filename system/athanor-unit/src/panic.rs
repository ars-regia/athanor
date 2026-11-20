//! A panic that leaves more than one line. Under `panic = "abort"` a panic ends the process
//! where it happens, and core dumps are off (doc_session.md SN8), so the hook writes what is
//! left to know first: the message, the place, the thread and a backtrace (#1352).
//!
//! A shipped binary is stripped, so its frames have no names: the backtrace is printed in
//! full, with each frame's address, and with the address `install` runs at, which places the
//! position-independent binary's load address for a symbolizer that has its debuginfo.

use std::backtrace::Backtrace;
use std::panic::{self, PanicHookInfo};
use std::thread;

/// Installs the hook. Call it first in `main`: through tracing once a subscriber is set,
/// to stderr before that, which a unit's journal reads either way.
pub fn install() {
    panic::set_hook(Box::new(|info| {
        let report = report(info, &Backtrace::force_capture());
        if tracing::dispatcher::has_been_set() {
            tracing::error!("{report}");
        } else {
            eprintln!("{report}");
        }
    }));
}

fn report(info: &PanicHookInfo<'_>, backtrace: &Backtrace) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(a payload that is not text)");
    let place = info
        .location()
        .map_or_else(|| "an unknown place".to_owned(), ToString::to_string);
    let current = thread::current();
    let name = current.name().unwrap_or("(unnamed)");
    let anchor = install as fn() as usize;
    format!(
        "panic in thread {name} at {place}: {message}\n\
         backtrace, with athanor_unit::panic::install at {anchor:#x}:\n{backtrace:#}"
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn the_report_holds_the_message_the_place_and_the_thread() {
        let seen = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&seen);
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            *sink.lock().expect("lock") = report(info, &Backtrace::disabled());
        }));
        let ended = thread::Builder::new()
            .name("worker".to_owned())
            .spawn(|| panic!("index {} out of range", 7))
            .expect("spawn")
            .join();
        panic::set_hook(previous);
        assert!(ended.is_err());
        let report = seen.lock().expect("lock").clone();
        assert!(report.starts_with("panic in thread worker at "), "{report}");
        assert!(report.contains("panic.rs:"), "{report}");
        assert!(report.contains(": index 7 out of range\n"), "{report}");
        let anchor = format!("install at {:#x}:", install as fn() as usize);
        assert!(report.contains(&anchor), "{report}");
    }
}
