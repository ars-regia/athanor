//! The session lock of doc_lock_and_prompts.md (LP2). It starts in the order of LP15 and of the
//! plan of LP18 step 2 (task 5): the locale, the confinement while the process has one thread,
//! the logind thread, then Sal's loop. This build locks nothing yet: it does not drive the state
//! machine, which waits for the screen (task 6), and says so. It ends non-zero on logind's fatal
//! events, so the unit's restart brings it back (LP10).

use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver};

use athanor_lock::confine::confine;
use athanor_lock::logind::{self, Call, Report};
use athanor_lock::state::Event;
use athanor_lock::unlock::Source;
use athanor_sal::{Application, Shell, SurfaceId};
use tokio::sync::mpsc::UnboundedSender;

fn main() -> ExitCode {
    // The time locale of this thread, which draws the screen and formats its dates.
    if let Err(error) = athanor_i18n::use_time_locale() {
        eprintln!(
            "<4>athanor-lock: cannot load the time locale ({error}); dates are formatted in the C locale"
        );
    }
    // Before any other thread, and failing closed: the lock never runs unconfined (LP15).
    if let Err(error) = confine(std::env::var_os("XDG_RUNTIME_DIR").as_deref()) {
        eprintln!(
            "<3>athanor-lock: cannot confine the process, refusing to run unconfined: {error}"
        );
        return ExitCode::FAILURE;
    }
    let (source, ping, _watch) = match Source::new() {
        Ok(source) => source,
        Err(error) => {
            eprintln!("<3>athanor-lock: cannot make the main loop's source: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (sender, reports) = channel();
    // After the confinement, which the thread inherits (plan decision M3).
    let calls = match logind::spawn(logind::SYSTEM_BUS, sender, ping) {
        Ok(calls) => calls,
        Err(error) => {
            eprintln!("<3>athanor-lock: cannot start the logind thread: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "<4>athanor-lock: this build does not lock the session yet (doc_lock_and_prompts.md, LP18 step 2)"
    );
    let lock = Lock {
        reports,
        _calls: calls,
    };
    match athanor_sal::run_with(lock, source) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("<3>athanor-lock: {error}");
            ExitCode::FAILURE
        }
    }
}

struct Lock {
    reports: Receiver<Report>,
    /// Held for the life of the loop: dropping it ends the logind thread.
    _calls: UnboundedSender<Call>,
}

impl Application for Lock {
    fn event(&mut self, _shell: &mut Shell, _surface: SurfaceId, _event: athanor_sal::Event) {}

    fn turn(&mut self, shell: &mut Shell) {
        for report in self.reports.try_iter() {
            if let Report::Event(Event::Fatal(fatal)) = report {
                shell.fail(format!("logind: {fatal:?}").into());
            }
        }
    }
}
