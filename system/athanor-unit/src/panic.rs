//! A panic that leaves more than one line. Under `panic = "abort"` a panic ends the process
//! where it happens, and core dumps are off (doc_session.md SN8), so the hook writes what is
//! left to know first: the message, the place, the thread and a backtrace (#1352).
//!
//! A shipped binary is stripped, so its frames have no names: the backtrace is printed in
//! full, with each frame's address, and with the address `install` runs at, which places the
//! position-independent binary's load address for a symbolizer that has its debuginfo.
//!
//! Every line goes to the journal at err priority, and the message and the place come again
//! last: a reader that keeps only the tail, the 50 journal lines of a crash report (SN8) or the
//! last 4096 bytes of the preview helper's stderr (`athanor-preview`), still has them.

use std::backtrace::Backtrace;
use std::panic::{self, PanicHookInfo};
use std::thread;

/// Installs the hook. Call it first in `main`: through tracing when the subscriber takes an
/// error from this module, to stderr otherwise (none set yet, or a `RUST_LOG` that drops it),
/// which a unit's journal reads either way.
pub fn install() {
    panic::set_hook(Box::new(|info| {
        emit(&report(info, &Backtrace::force_capture()))
    }));
}

/// Writes `report` a line at a time, each at err priority: one tracing event per line, or each
/// line after `<3>` on stderr, the prefix journald reads as the priority (SyslogLevelPrefix=).
fn emit(report: &str) {
    if through_tracing() {
        for line in report.lines() {
            tracing::error!("{line}");
        }
    } else {
        eprint!("{}", for_stderr(report));
    }
}

fn for_stderr(report: &str) -> String {
    report.lines().map(|line| format!("<3>{line}\n")).collect()
}

fn through_tracing() -> bool {
    tracing::enabled!(tracing::Level::ERROR)
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
    // "panicked at", as the default hook writes it: the rig and acceptance checks look for it.
    let head = format!("thread {name} panicked at {place}: {message}");
    format!(
        "{head}\n\
         backtrace, with athanor_unit::panic::install at {anchor:#x}:\n{backtrace:#}\n\
         {head}"
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
        assert!(report.starts_with("thread worker panicked at "), "{report}");
        assert!(report.contains("panic.rs:"), "{report}");
        assert!(report.contains(": index 7 out of range\n"), "{report}");
        let anchor = format!("install at {:#x}:", install as fn() as usize);
        assert!(report.contains(&anchor), "{report}");
        let last = report.lines().last().unwrap_or_default();
        assert!(last.starts_with("thread worker panicked at "), "{report}");
        assert!(last.ends_with(": index 7 out of range"), "{report}");
    }

    #[test]
    fn every_line_of_the_report_is_at_err_priority() {
        let report = "thread main panicked at a.rs:1:2: no\n   0: 0x1 - <unknown>\nthread main panicked at a.rs:1:2: no";
        let text = for_stderr(report);
        assert_eq!(text.lines().count(), 3);
        assert!(text.lines().all(|line| line.starts_with("<3>")), "{text}");

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&buffer);
        let make = move || Sink(Arc::clone(&sink));
        tracing::subscriber::with_default(crate::journal::subscriber(make), || emit(report));
        let logged = String::from_utf8(buffer.lock().expect("lock").clone()).expect("utf-8");
        assert_eq!(logged, text);
    }

    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_filter_that_drops_the_report_leaves_it_to_stderr() {
        let logs_under = |filter: &str| {
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
                .with_writer(std::io::sink)
                .finish();
            tracing::subscriber::with_default(subscriber, through_tracing)
        };
        assert!(logs_under("error"));
        assert!(!logs_under("off"));
        assert!(!logs_under("athanor_bar=debug"));
    }
}
